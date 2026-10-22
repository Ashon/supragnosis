//! The joining machine's side of pairing (docs/pairing.md Sections 4.3, 4.4 and 4.6): `join`, which
//! reads a hub's code, asks to join, shows the words and writes what the hub's signed answer grants,
//! and `hub leave`.
//!
//! What a join records beside `supragnosis.toml` lives in `~/.supragnosis/hubs/`, keyed by the hub's
//! node id (D15): the receive list, what the join added to the sharing lists, and the hub's pinned
//! certificate. An open join keeps its one-time key there too, so `join` run again with the same
//! code asks as the same request.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use supragnosis_core::NodeIdentity;

use crate::invite::JoinRequest;
use crate::pair::{self, hex, hex32, CertCheck, Code, OneTime};

/// Where a join keeps its records, and the configuration it edits.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `~/.supragnosis`
    pub base: PathBuf,
    pub config: PathBuf,
}

impl Paths {
    pub fn default_paths() -> Paths {
        Paths { base: crate::fed::fed_base_dir(), config: crate::fed::config_path() }
    }

    fn hubs(&self) -> PathBuf {
        self.base.join("hubs")
    }

    fn joining(&self) -> PathBuf {
        self.hubs().join("joining.toml")
    }

    fn sidecar(&self, hub: &str) -> PathBuf {
        self.hubs().join(format!("{hub}.toml"))
    }

    fn pem(&self, hub: &str) -> PathBuf {
        self.hubs().join(format!("{hub}.pem"))
    }
}

/// An open join (Section 4.3.2): the code it belongs to, the one-time key, this machine's nonce, and
/// the hub's once it has it - written before this machine reveals its own - and what the person
/// chose, so a join run again after the hub's Allow finishes without asking twice.
#[derive(Debug, Serialize, Deserialize)]
struct Joining {
    invitation: String,
    expires: u64,
    onetime: String,
    nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hub_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    chosen: Option<Chosen>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Chosen {
    hub_name: String,
    send: Vec<String>,
    serve: bool,
}

/// What a join recorded beside `supragnosis.toml` (Section 4.3, D15).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sidecar {
    pub name: String,
    pub hub_key: String,
    pub url: String,
    /// Every invited workspace (D12): what this machine pulls, whatever the hub would admit.
    pub receive: Vec<String>,
    /// What the join added to `share_workspaces` and `serve_workspaces`, so leaving removes those
    /// and nothing the person wrote.
    pub sent: Vec<String>,
    pub served: Vec<String>,
    /// The `[sync]` keys the join created - `sync` itself among them - which leaving removes once
    /// they are empty again.
    #[serde(default)]
    pub created: Vec<String>,
    #[serde(default)]
    pub vouched: Vec<pair::Vouched>,
}

/// What the person chose when the hub's preview was shown.
pub struct Choices {
    pub send: Vec<String>,
    pub serve: bool,
}

/// What a join shows the person and asks them - the terminal's prompts, or the app's sheet.
pub trait Person {
    fn preview(
        &mut self,
        hub: &str,
        workspaces: &[String],
        serves_principals: bool,
    ) -> Result<Choices>;
    fn words(&mut self, hub: &str, words: &[&str]);
}

/// What a join wrote.
#[derive(Debug, Clone, Serialize)]
pub struct Joined {
    pub hub_name: String,
    pub hub: String,
    pub url: String,
    pub receive: Vec<String>,
    pub sent: Vec<String>,
    pub served: Vec<String>,
}

fn read_toml<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn write_toml<T: Serialize>(path: &std::path::Path, value: &T) -> Result<()> {
    crate::write_secret(path, toml::to_string(value)?.as_bytes())
}

/// Who may join (D6, PA9): not a machine that already syncs to a hub, and not one that admits a
/// node of its own.
pub fn may_join(paths: &Paths) -> Result<()> {
    let Some(fc) = crate::fed::load_at(&paths.config)? else {
        return Ok(());
    };
    if let Some(link) = fc.sync.links().0.first() {
        anyhow::bail!(
            "This machine already syncs with {}. In this version a machine syncs with one other; \
             `supragnosis hub leave` stops that first.",
            link.url
        );
    }
    if let Some(entry) = fc.server.as_ref().and_then(|s| s.allowlist.first()) {
        anyhow::bail!(
            "This machine admits a machine of its own ({}), so it does not join another in this \
             version.",
            entry.node_id
        );
    }
    Ok(())
}

/// The hub at the first of the code's addresses that answers, checked as the code says (Section
/// 4.3), with its preview.
async fn reach(code: &Code) -> Result<(String, reqwest::Client, serde_json::Value)> {
    use supragnosis_sync::http::{join_client, JoinTls, NOT_THE_CODES_HUB};
    let client = match &code.check {
        CertCheck::Fingerprint(pin) => {
            let pin = *pin;
            join_client(JoinTls::Pinned(Arc::new(move |der| pair::sha256(der) == pin)))?
        }
        CertCheck::ByName => join_client(JoinTls::ByName)?,
    };
    let mut other_certificate = Vec::new();
    for address in &code.addresses {
        let sent = client
            .post(format!("https://{address}/pair/preview"))
            .json(&json!({"secret": hex(&code.secret)}))
            .send()
            .await;
        match sent {
            Ok(r) if r.status().is_success() => {
                let preview = r.json().await.context("the hub's preview is not JSON")?;
                return Ok((address.clone(), client, preview));
            }
            Ok(r) => tracing::warn!(%address, status = %r.status(), "the hub refused the preview"),
            Err(e) if format!("{e:?}").contains(NOT_THE_CODES_HUB) => {
                other_certificate.push(address.clone())
            }
            Err(e) => tracing::warn!(%address, error = %e, "the hub did not answer there"),
        }
    }
    if !other_certificate.is_empty() {
        anyhow::bail!(
            "This is not the machine that made the code: {} presented another certificate.",
            other_certificate.join(", ")
        );
    }
    let port = code.addresses.first().map(|a| crate::invite::port_of(a)).unwrap_or(7420);
    anyhow::bail!(
        "Could not reach the hub at {}. Check that it is on, on a network this machine reaches, and \
         that its firewall - the macOS firewall, or ufw or firewalld on a server - lets port {port} \
         in.",
        code.addresses.join(", ")
    )
}

fn said(answer: &serde_json::Value) -> String {
    answer["said"].as_str().unwrap_or("The hub ended the join.").to_string()
}

/// `join` (Section 4.3): asks the hub the code names to admit this machine, shows the words while it
/// waits for the hub's Allow, and on its signed answer writes what it grants. The person's ticks
/// count only where the answer agrees.
pub async fn join(
    paths: &Paths,
    me: &NodeIdentity,
    name: &str,
    code: &Code,
    person: &mut dyn Person,
    poll: Duration,
) -> Result<Joined> {
    anyhow::ensure!(
        code.expires > crate::invite::now(),
        "This code has expired. Make a new code on the hub."
    );
    may_join(paths)?;
    crate::private_dir(&paths.hubs())?;
    let invitation = pair::secret_hash(&code.secret);
    let mut open = match read_toml::<Joining>(&paths.joining())? {
        Some(j) if j.invitation == invitation => j,
        _ => Joining {
            invitation,
            expires: code.expires,
            onetime: hex(&pair::random32()?),
            nonce: hex(&pair::random32()?),
            hub_nonce: None,
            chosen: None,
        },
    };
    write_toml(&paths.joining(), &open)?;
    let end = |why: String| -> anyhow::Error {
        let _ = std::fs::remove_file(paths.joining());
        anyhow::anyhow!(why)
    };
    let onetime = OneTime::from_secret(hex32(&open.onetime).context("the kept one-time key")?);
    let nonce = hex32(&open.nonce).context("the kept nonce")?;
    let node_key = hex32(&me.public_key_hex()).context("this machine's key")?;
    let commitment = pair::commitment(&node_key, &onetime.public(), &nonce);

    let (address, client, preview) = reach(code).await?;
    // A join run again keeps what the person chose: the hub may have allowed it since, and then its
    // preview says the code is used while the answer waits for this request.
    let chosen = match open.chosen.clone() {
        Some(kept) => kept,
        None => {
            if preview["state"] != "open" {
                return Err(end(said(&preview)));
            }
            let hub_name = preview["hub_name"].as_str().unwrap_or("the hub").to_string();
            let offered: Vec<String> = serde_json::from_value(preview["workspaces"].clone())?;
            let serves = preview["serves_principals"] == true;
            let choices = person.preview(&hub_name, &offered, serves)?;
            let kept = Chosen { hub_name, send: choices.send, serve: choices.serve };
            open.chosen = Some(kept.clone());
            write_toml(&paths.joining(), &open)?;
            kept
        }
    };
    let (hub_name, choices) = (chosen.hub_name, Choices { send: chosen.send, serve: chosen.serve });
    let url = format!("https://{address}");
    let ask = |reveal: bool| JoinRequest {
        secret: hex(&code.secret),
        node_key: me.public_key_hex(),
        name: name.to_string(),
        onetime_key: hex(&onetime.public()),
        commitment: hex(&commitment),
        nonce: reveal.then(|| hex(&nonce)),
    };
    let call = |req: JoinRequest| {
        let (client, url) = (client.clone(), format!("{url}/pair/join"));
        async move {
            let r = client.post(url).json(&req).send().await?.error_for_status()?;
            anyhow::Ok(r.json::<serde_json::Value>().await?)
        }
    };
    // The hub's nonce is kept before this machine reveals its own (Section 4.3.2).
    let hub_nonce = match open.hub_nonce.clone() {
        Some(kept) => kept,
        None => {
            let first = call(ask(false)).await?;
            if first["state"] != "pending" {
                return Err(end(said(&first)));
            }
            let theirs = first["hub_nonce"].as_str().context("the hub sent no nonce")?.to_string();
            open.hub_nonce = Some(theirs.clone());
            write_toml(&paths.joining(), &open)?;
            theirs
        }
    };
    let words = pair::words(
        &code.secret,
        &node_key,
        &onetime.public(),
        &nonce,
        &hex32(&hub_nonce).context("the hub's nonce")?,
    );
    let mut shown = false;
    let answer = loop {
        let reply = match call(ask(true)).await {
            Ok(reply) => reply,
            // The hub keeps a request and its answer until the code expires, so a dropped
            // connection is asked again until then.
            Err(e) if crate::invite::now() <= code.expires => {
                tracing::warn!(error = %format!("{e:#}"), "asking the hub again");
                tokio::time::sleep(poll).await;
                continue;
            }
            Err(e) => return Err(e),
        };
        match reply["state"].as_str() {
            Some("pending") => {
                if reply["hub_nonce"].as_str() != Some(hub_nonce.as_str()) {
                    return Err(end(
                        "The hub's answer changed during the join, so this join stops. Make a \
                         new code on the hub."
                            .into(),
                    ));
                }
                if !shown {
                    person.words(&hub_name, &words);
                    shown = true;
                }
                tokio::time::sleep(poll).await;
            }
            Some("allowed") => break reply["answer"].clone(),
            _ => return Err(end(said(&reply))),
        }
    };
    let signed: pair::SignedAnswer = serde_json::from_value(answer).context("the hub's answer")?;
    let expect = pair::Expected {
        code,
        joining_node: &me.node_id(),
        onetime_key: &onetime.public(),
        joiner_nonce: &nonce,
        hub_nonce: &hex32(&hub_nonce).unwrap_or_default(),
    };
    let body = pair::verify_answer(&expect, &signed).map_err(|e| end(format!("{e:#}")))?;
    let credential =
        String::from_utf8(onetime.open(&body.credential).map_err(|e| end(format!("{e:#}")))?)
            .map_err(|_| end("the credential is not text".into()))?;
    // Ticks count only where the signed answer agrees, since the preview is not signed.
    let send: Vec<String> =
        choices.send.into_iter().filter(|w| body.workspaces.contains(w)).collect();
    let serve = choices.serve && body.serves_principals;
    let hub = code.hub.clone();
    let pem_path = match &body.certificate {
        Some(pem) => {
            let path = paths.pem(&hub);
            crate::write_secret(&path, pem.as_bytes())?;
            Some(path.display().to_string())
        }
        None => None,
    };
    let (sent, served, created) = crate::fed::edit_config_at(&paths.config, |text| {
        let fc: crate::fed::FileConfig =
            toml::from_str(text).context("parsing supragnosis.toml")?;
        anyhow::ensure!(
            fc.sync.links().0.is_empty(),
            "this machine began syncing with another hub during the join"
        );
        if let Some(kept) = fc.sync.origin_keys.get(&hub) {
            anyhow::ensure!(
                kept.eq_ignore_ascii_case(&body.hub_key),
                "supragnosis.toml already names another key for the hub {hub}"
            );
        }
        let mut doc: toml_edit::DocumentMut = text.parse().context("parsing supragnosis.toml")?;
        let mut created: Vec<String> = Vec::new();
        if doc.get("sync").is_none() {
            created.push("sync".into());
        }
        let sync = doc
            .entry("sync")
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_mut()
            .context("[sync] is not a table")?;
        for key in ["server", "origin_keys", "share_workspaces", "serve_workspaces"] {
            if !sync.contains_key(key) {
                created.push(key.into());
            }
        }
        let servers = sync
            .entry("server")
            .or_insert(toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new()))
            .as_array_of_tables_mut()
            .context("[sync] server is not an array of tables")?;
        let mut entry = toml_edit::Table::new();
        entry["url"] = toml_edit::value(url.as_str());
        entry["auth_token"] = toml_edit::value(credential.as_str());
        if let Some(pem) = &pem_path {
            entry["ca"] = toml_edit::value(pem.as_str());
        }
        servers.push(entry);
        sync.entry("origin_keys")
            .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
            .as_table_like_mut()
            .context("[sync] origin_keys is not a table")?
            .insert(&hub, toml_edit::value(body.hub_key.as_str()));
        let mut add = |key: &str, what: &[String]| -> Result<Vec<String>> {
            let list = sync
                .entry(key)
                .or_insert(toml_edit::value(toml_edit::Array::new()))
                .as_array_mut()
                .with_context(|| format!("[sync] {key} is not a list"))?;
            let mut added = Vec::new();
            for w in what {
                if !list.iter().any(|v| v.as_str() == Some(w)) {
                    list.push(w.as_str());
                    added.push(w.clone());
                }
            }
            Ok(added)
        };
        let sent = add("share_workspaces", &send)?;
        let served = match serve {
            true => add("serve_workspaces", &send)?,
            false => Vec::new(),
        };
        if !serve {
            created.retain(|k| k != "serve_workspaces");
        }
        let edited = doc.to_string();
        toml::from_str::<crate::fed::FileConfig>(&edited)
            .context("the joined configuration does not read back")?;
        Ok((edited, (sent, served, created)))
    })?;
    owner_only(&paths.config)?;
    write_toml(
        &paths.sidecar(&hub),
        &Sidecar {
            name: body.hub_name.clone(),
            hub_key: body.hub_key.clone(),
            url: url.clone(),
            receive: body.workspaces.clone(),
            sent: sent.clone(),
            served: served.clone(),
            created,
            vouched: body.vouched.clone(),
        },
    )?;
    let _ = std::fs::remove_file(paths.joining());
    tracing::info!(hub = %hub, url = %url, "joined a hub");
    Ok(Joined { hub_name: body.hub_name, hub, url, receive: body.workspaces, sent, served })
}

/// `supragnosis.toml` holds the credential once a join has written it: owner-only, wherever
/// SUPRAGNOSIS_CONFIG puts it (Section 4.3).
fn owner_only(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

/// The hubs this machine joined, by node id, with what each join recorded.
pub fn joined(paths: &Paths) -> Result<Vec<(String, Sidecar)>> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir(paths.hubs()) else {
        return Ok(out);
    };
    for entry in dir.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        if path.extension().and_then(|e| e.to_str()) != Some("toml") || stem == "joining" {
            continue;
        }
        if let Some(car) = read_toml::<Sidecar>(&path)? {
            out.push((stem, car));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// What this node pulls, where a join recorded it (PA12): the union of its joins' receive lists,
/// or `None` on a node no join set up. A sidecar that cannot be read is said, and pulls nothing
/// extra rather than everything.
pub fn receive_list(paths: &Paths) -> (Option<Vec<String>>, Option<String>) {
    match joined(paths) {
        Ok(hubs) if hubs.is_empty() => (None, None),
        Ok(hubs) => {
            let mut list: Vec<String> = Vec::new();
            for w in hubs.into_iter().flat_map(|(_, car)| car.receive) {
                if !list.contains(&w) {
                    list.push(w);
                }
            }
            (Some(list), None)
        }
        Err(e) => (
            Some(Vec::new()),
            Some(format!("the hubs this node joined could not be read ({e:#}), so it pulls only what it shares")),
        ),
    }
}

/// What leaving did.
#[derive(Debug, Clone, Serialize)]
pub struct Left {
    pub hub_name: String,
    /// Whether the hub answered and removed this machine itself (Section 4.6).
    pub hub_answered: bool,
}

/// `hub leave` (Section 4.6): asks the hub, with this machine's credential, to remove it, then
/// removes the entry, the key and the records the join wrote - and nothing the person wrote.
pub async fn leave(paths: &Paths) -> Result<Left> {
    let hubs = joined(paths)?;
    let Some((hub, car)) = hubs.into_iter().next() else {
        anyhow::bail!(
            "This machine joined no hub through pairing. A hub written into {} by hand is removed \
             there.",
            paths.config.display()
        );
    };
    let fc = crate::fed::load_at(&paths.config)?.unwrap_or_default();
    let entry = fc.sync.server.iter().find(|e| e.url == car.url).cloned();
    let mut hub_answered = false;
    if let Some(entry) = &entry {
        let builder = reqwest::Client::builder().timeout(Duration::from_secs(20));
        let builder = match &entry.ca {
            Some(ca) => {
                let pem = std::fs::read(ca).with_context(|| format!("reading {}", ca.display()))?;
                builder.use_preconfigured_tls(supragnosis_sync::http::tls_trusting(&pem)?)
            }
            None => builder,
        };
        let sent = builder
            .build()?
            .post(format!("{}/pair/leave", car.url))
            .bearer_auth(&entry.auth_token)
            .send()
            .await;
        hub_answered = sent.is_ok_and(|r| r.status().is_success());
    }
    crate::fed::edit_config_at(&paths.config, |text| {
        let mut doc: toml_edit::DocumentMut = text.parse().context("parsing supragnosis.toml")?;
        if let Some(sync) = doc.get_mut("sync").and_then(|s| s.as_table_mut()) {
            if let Some(servers) = sync.get_mut("server").and_then(|s| s.as_array_of_tables_mut()) {
                servers.retain(|t| t.get("url").and_then(|u| u.as_str()) != Some(car.url.as_str()));
            }
            if let Some(keys) = sync.get_mut("origin_keys").and_then(|k| k.as_table_like_mut()) {
                if keys.get(&hub).and_then(|k| k.as_str()) == Some(car.hub_key.as_str()) {
                    keys.remove(&hub);
                }
            }
            for (key, added) in [("share_workspaces", &car.sent), ("serve_workspaces", &car.served)]
            {
                if let Some(list) = sync.get_mut(key).and_then(|l| l.as_array_mut()) {
                    list.retain(|v| !v.as_str().is_some_and(|w| added.iter().any(|a| a == w)));
                }
            }
            // What the join created goes once it is empty again; what the person wrote stays.
            for key in car.created.iter().filter(|k| *k != "sync") {
                let empty = match sync.get(key) {
                    Some(toml_edit::Item::ArrayOfTables(a)) => a.is_empty(),
                    Some(item) => {
                        item.as_array().is_some_and(|a| a.is_empty())
                            || item.as_table_like().is_some_and(|t| t.is_empty())
                    }
                    None => false,
                };
                if empty {
                    sync.remove(key);
                }
            }
        }
        if car.created.iter().any(|k| k == "sync")
            && doc.get("sync").and_then(|s| s.as_table()).is_some_and(|t| t.is_empty())
        {
            doc.remove("sync");
        }
        Ok((doc.to_string(), ()))
    })?;
    let _ = std::fs::remove_file(paths.sidecar(&hub));
    let _ = std::fs::remove_file(paths.pem(&hub));
    tracing::info!(hub = %hub, answered = hub_answered, "left a hub");
    Ok(Left { hub_name: car.name, hub_answered })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invite::tests_support::{Hub, HUB_SECRET};
    use crate::invite::{self, Pairing};
    use supragnosis_engine::{Engine, EntityInput, ObserveInput};
    use supragnosis_store::InMemoryStore;
    use supragnosis_sync::SyncNode;

    /// The test's person: ticks what it was told to, and keeps the words it was shown.
    struct Scripted {
        send: Vec<String>,
        serve: bool,
        offered: Vec<String>,
        words: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl Person for Scripted {
        fn preview(&mut self, _hub: &str, workspaces: &[String], _serves: bool) -> Result<Choices> {
            self.offered = workspaces.to_vec();
            Ok(Choices { send: self.send.clone(), serve: self.serve })
        }

        fn words(&mut self, _hub: &str, words: &[&str]) {
            *self.words.lock().unwrap() = words.iter().map(|w| w.to_string()).collect();
        }
    }

    /// A hub serving its pairing calls and the sync API on loopback, over TLS with the
    /// certificate it made, and the port it listens on.
    async fn serve(hub: &Hub) -> (Arc<crate::hub::HubListener>, u16) {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let config = hub.config().replace("127.0.0.1:0", &format!("127.0.0.1:{port}"));
        std::fs::write(&hub.paths.config, config).unwrap();
        let node = Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes(HUB_SECRET)));
        let peers = Arc::new(supragnosis_sync::http::PeerDirectory::new(
            Vec::new(),
            node.node_id(),
            &node.public_key_hex(),
        ));
        let pairing = Arc::new(hub.pairing());
        let listener = Arc::new(crate::hub::HubListener::new(
            hub.store.clone(),
            node,
            Default::default(),
            peers,
            pairing,
            Default::default(),
        ));
        listener.follow();
        let state = listener.refresh().await;
        assert_eq!(state["listening"], true, "{state}");
        (listener, port)
    }

    fn invite_at(p: &Pairing, port: u16, workspaces: &[&str]) -> Code {
        let pem = std::fs::read(p.config_path().parent().unwrap().join("hub/cert.pem")).unwrap();
        let der = supragnosis_sync::http::certificate_der(&pem).unwrap();
        let made = p
            .invite(
                workspaces.iter().map(|w| w.to_string()).collect(),
                vec![format!("127.0.0.1:{port}")],
                CertCheck::Fingerprint(pair::sha256(&der)),
                invite::now(),
            )
            .unwrap();
        Code::decode(&made.code).unwrap()
    }

    /// The hub's person: Allow once the words are shown, for the request shown.
    fn allow_when_asked(p: Arc<Pairing>) -> tokio::task::JoinHandle<serde_json::Value> {
        tokio::task::spawn_blocking(move || {
            for _ in 0..400 {
                let shown = crate::invite::tests_support::shown(&p, invite::now());
                if !shown.words.is_empty() {
                    return p.allow(&shown, invite::now()).unwrap();
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            panic!("no request reached its words");
        })
    }

    struct Machine {
        dir: PathBuf,
        paths: Paths,
        me: NodeIdentity,
        secret: [u8; 32],
    }

    impl Machine {
        fn new(name: &str, seed: u8) -> Machine {
            let dir = std::env::temp_dir().join(format!("sg-joiner-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let paths = Paths { base: dir.clone(), config: dir.join("supragnosis.toml") };
            std::fs::write(&paths.config, "# the person's own line\nhost_label = \"MacBook\"\n")
                .unwrap();
            Machine {
                dir,
                paths,
                me: NodeIdentity::from_secret_bytes([seed; 32]),
                secret: [seed; 32],
            }
        }

        fn config(&self) -> String {
            std::fs::read_to_string(&self.paths.config).unwrap()
        }
    }

    impl Drop for Machine {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn observe(engine: &Engine, workspace: &str, entity: &str) {
        engine
            .observe(ObserveInput {
                content: format!("{entity} is recorded in {workspace}"),
                workspace: Some(workspace.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput { name: entity.into(), kind: None, description: None }],
                relations: vec![],
            })
            .unwrap();
    }

    /// The sync context a joined machine's daemon builds from what the join wrote.
    fn context(m: &Machine) -> supragnosis_mcp::SyncContext {
        let fc = crate::fed::load_at(&m.paths.config).unwrap().unwrap();
        let (servers, notes) = fc.sync.links();
        assert!(notes.is_empty(), "{notes:?}");
        let mut origin_keys = fc.sync.origin_keys.clone();
        origin_keys.insert(m.me.node_id(), m.me.public_key_hex());
        supragnosis_mcp::SyncContext {
            node: Arc::new(SyncNode::new(NodeIdentity::from_secret_bytes(m.secret))),
            share_workspaces: fc.sync.share_workspaces.clone(),
            receive: receive_list(&m.paths).0,
            serve_workspaces: fc.sync.serve_workspaces.clone(),
            servers,
            config_notes: Vec::new(),
            surfaces: Default::default(),
            origin_keys,
            peer_registry: None,
            round: Default::default(),
            last_round: Default::default(),
            listener: Default::default(),
        }
    }

    /// docs/pairing.md Section 10 step 3b: a hub invites, the person allows on it, a machine joins
    /// from the code - ticking one workspace it is offered and one it is not - and the two then
    /// keep the invited workspaces in step by the joined machine's round alone: what it sends goes
    /// to the hub, and what it receives comes back, received-only workspaces included (PA5, PA12).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_machine_joins_and_both_sync_with_no_call() {
        let hub = Hub::new("join-two");
        let (listener, port) = serve(&hub).await;
        let code = invite_at(&listener.pairing, port, &["acme", "docs"]);
        let m = Machine::new("two", 21);
        let words = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut person = Scripted {
            send: vec!["acme".into(), "secret".into()],
            serve: true,
            offered: Vec::new(),
            words: words.clone(),
        };
        let allowed = allow_when_asked(listener.pairing.clone());
        let joined =
            join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30))
                .await
                .expect("joins");
        let hub_side = allowed.await.unwrap();
        assert_eq!(person.offered, vec!["acme".to_string(), "docs".to_string()]);
        assert_eq!(words.lock().unwrap().len(), 3, "the words were shown");
        assert_eq!(hub_side["joined"]["name"], "MacBook");
        assert_eq!(joined.receive, vec!["acme".to_string(), "docs".to_string()]);
        assert_eq!(
            joined.sent,
            vec!["acme".to_string()],
            "a tick the hub never offered is not sent"
        );
        assert!(joined.served.is_empty(), "a hub serving no one gets no serve consent");
        let text = m.config();
        assert!(text.starts_with("# the person's own line\n"), "{text}");
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&m.paths.config).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the file holding the credential is owner-only");
        assert!(!m.paths.joining().exists(), "the open join is gone");
        let sidecar = std::fs::read_to_string(m.paths.sidecar(&code.hub)).unwrap();
        let fc = crate::fed::load_at(&m.paths.config).unwrap().unwrap();
        assert!(
            !sidecar.contains(&fc.sync.server[0].auth_token),
            "the credential lives in one file"
        );

        // The rounds a daemon runs on its own.
        let hub_engine = Engine::new(hub.mem.clone(), "hub", "acme");
        observe(&hub_engine, "acme", "Ledger");
        observe(&hub_engine, "docs", "Guide");
        let mine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "mac", "acme"));
        observe(&mine, "acme", "Invoice");
        let ctx = context(&m);
        let round = supragnosis_mcp::sync_round(&mine, &ctx).await;
        assert_eq!(round["ok"], true, "{round}");
        let has = |e: &Engine, ws: &str, name: &str| {
            e.get_entity(&supragnosis_core::Entity::make_id(ws, name)).unwrap().is_some()
        };
        assert!(has(&mine, "acme", "Ledger"), "the hub's write reached the joined machine");
        assert!(has(&mine, "docs", "Guide"), "and so did a workspace it only receives");
        hub_engine.reproject(Some("acme")).unwrap();
        assert!(
            has(&hub_engine, "acme", "Invoice"),
            "the joined machine's write reached the hub"
        );
    }

    /// A code whose pin is not the certificate the address presents ends there, said as such; and
    /// a machine that already syncs with a hub, or admits one, does not join (PA3, PA9).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_refuses_another_certificate_and_a_chain() {
        let hub = Hub::new("join-cert");
        let (listener, port) = serve(&hub).await;
        let code = invite_at(&listener.pairing, port, &["acme"]);
        let m = Machine::new("cert", 22);
        let words = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut person = Scripted { send: vec![], serve: false, offered: vec![], words };
        let wrong = Code { check: CertCheck::Fingerprint([7; 32]), ..code.clone() };
        let e = join(&m.paths, &m.me, "MacBook", &wrong, &mut person, Duration::from_millis(30))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("not the machine that made the code"), "{e}");
        let expired = Code { expires: invite::now() - 1, ..code.clone() };
        let e = join(&m.paths, &m.me, "MacBook", &expired, &mut person, Duration::from_millis(30))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("expired"), "{e}");
        std::fs::write(
            &m.paths.config,
            "[sync]\n[[sync.server]]\nurl = \"https://other.example:7420\"\nauth_token = \"t\"\n",
        )
        .unwrap();
        let e = join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("already syncs with https://other.example:7420"), "{e}");
        std::fs::write(
            &m.paths.config,
            "[server]\nlisten = \"127.0.0.1:1\"\n[[server.allowlist]]\nnode_id = \"abc\"\npublic_key_hex = \"00\"\nbearer_hash = \"00\"\nshared_workspaces = []\n",
        )
        .unwrap();
        let e = may_join(&m.paths).unwrap_err().to_string();
        assert!(e.contains("admits a machine of its own"), "a hub does not join another: {e}");
    }

    /// `join` run again with the same code asks as the same request - the invitation stays open -
    /// and a hub nonce other than the one kept ends the join (Section 4.3.2).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_join_run_again_is_the_same_request_and_a_changed_nonce_ends_it() {
        let hub = Hub::new("join-again");
        let (listener, port) = serve(&hub).await;
        let code = invite_at(&listener.pairing, port, &["acme"]);
        let m = Machine::new("again", 23);
        // A first run that stopped after the hub answered "pending" with its nonce.
        let first = Joining {
            invitation: pair::secret_hash(&code.secret),
            expires: code.expires,
            onetime: hex(&pair::random32().unwrap()),
            nonce: hex(&pair::random32().unwrap()),
            hub_nonce: Some(hex(&[9; 32])),
            chosen: None,
        };
        crate::private_dir(&m.paths.hubs()).unwrap();
        write_toml(&m.paths.joining(), &first).unwrap();
        let words = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut person = Scripted { send: vec![], serve: false, offered: vec![], words };
        // The hub never sent that nonce: the request it holds is not this one, so the join ends.
        let e = join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("Make a new code"), "{e}");
        assert!(!m.paths.joining().exists(), "an ended join leaves nothing to reuse");

        // A run that stops before Allow and runs again keeps the request, and joins.
        let code = invite_at(&listener.pairing, port, &["acme"]);
        let words = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut person =
            Scripted { send: vec![], serve: false, offered: vec![], words: words.clone() };
        let stopped = tokio::time::timeout(
            Duration::from_millis(600),
            join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30)),
        )
        .await;
        assert!(stopped.is_err(), "it was waiting for Allow");
        let kept: Joining = read_toml(&m.paths.joining()).unwrap().expect("kept");
        assert!(kept.hub_nonce.is_some() && kept.chosen.is_some());
        // Allowed on the hub while no `join` ran; the next run finds the answer waiting.
        allow_when_asked(listener.pairing.clone()).await.unwrap();
        let mut person = Scripted { send: vec![], serve: false, offered: vec![], words };
        join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30))
            .await
            .expect("the same request, allowed");
        assert!(person.offered.is_empty(), "nothing was asked twice");
    }

    /// An answer whose signature does not verify under the key the code names is not acted on:
    /// nothing is written, and the join ends (PA14).
    #[tokio::test(flavor = "multi_thread")]
    async fn a_tampered_answer_writes_nothing() {
        let hub = Hub::new("join-tamper");
        let (listener, port) = serve(&hub).await;
        let code = invite_at(&listener.pairing, port, &["acme"]);
        let m = Machine::new("tamper", 25);
        let before = m.config();
        let words = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut person =
            Scripted { send: vec!["acme".into()], serve: false, offered: vec![], words };
        // Asked, then stopped while it waited for Allow.
        let waited = tokio::time::timeout(
            Duration::from_millis(600),
            join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30)),
        )
        .await;
        assert!(waited.is_err(), "it was waiting for Allow");
        // Allowed, and the kept answer's signature bent: the right length, the wrong bytes.
        allow_when_asked(listener.pairing.clone()).await.unwrap();
        let invites = hub.paths.hub.join("invites.toml");
        let text = std::fs::read_to_string(&invites).unwrap();
        let at = text.find("signature = \"").unwrap() + 13;
        let mut bent = text.clone();
        bent.replace_range(at..at + 2, if &text[at..at + 2] == "00" { "01" } else { "00" });
        crate::write_secret(&invites, bent.as_bytes()).unwrap();
        let e = join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30))
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("signature does not verify"), "{e}");
        assert_eq!(m.config(), before, "nothing was written");
        assert!(joined(&m.paths).unwrap().is_empty());
    }

    /// `hub leave` asks the hub to remove this machine, then removes only what the join wrote; and a
    /// hub that removed it first is said not to have answered, and the rest happens anyway.
    #[tokio::test(flavor = "multi_thread")]
    async fn leaving_removes_only_what_the_join_wrote() {
        let hub = Hub::new("join-leave");
        let (listener, port) = serve(&hub).await;
        let code = invite_at(&listener.pairing, port, &["acme"]);
        let m = Machine::new("leave", 24);
        std::fs::write(
            &m.paths.config,
            "# the person's own line\n[sync]\nshare_workspaces = [\"mine\"]\n",
        )
        .unwrap();
        let words = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut person =
            Scripted { send: vec!["acme".into()], serve: false, offered: vec![], words };
        let allowed = allow_when_asked(listener.pairing.clone());
        join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30))
            .await
            .expect("joins");
        allowed.await.unwrap();
        let left = leave(&m.paths).await.expect("leaves");
        assert!(left.hub_answered, "the hub removed this machine");
        let peers = listener.pairing.peers().unwrap();
        assert!(peers.joined.is_empty() && peers.removed.len() == 1);
        let fc = crate::fed::load_at(&m.paths.config).unwrap().unwrap();
        assert!(fc.sync.server.is_empty() && fc.sync.origin_keys.is_empty());
        assert_eq!(
            fc.sync.share_workspaces,
            vec!["mine".to_string()],
            "what the person wrote stays"
        );
        assert!(m.config().starts_with("# the person's own line\n"));
        assert!(joined(&m.paths).unwrap().is_empty() && !m.paths.pem(&code.hub).exists());
        assert!(leave(&m.paths).await.is_err(), "nothing left to leave");
        assert!(!m.config().contains("origin_keys"), "a table the join made goes once empty");

        // A file with no [sync] of its own is as it was after a join and a leave.
        let m = Machine::new("leave-bare", 26);
        let before = m.config();
        let code = invite_at(&listener.pairing, port, &["acme"]);
        let words = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut person =
            Scripted { send: vec!["acme".into()], serve: false, offered: vec![], words };
        let allowed = allow_when_asked(listener.pairing.clone());
        join(&m.paths, &m.me, "MacBook", &code, &mut person, Duration::from_millis(30))
            .await
            .expect("joins again");
        allowed.await.unwrap();
        leave(&m.paths).await.expect("leaves");
        assert_eq!(m.config().trim_end(), before.trim_end(), "{}", m.config());
    }
}
