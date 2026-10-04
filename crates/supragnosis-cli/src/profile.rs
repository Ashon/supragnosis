//! Server profiles - which supragnosis server this machine's clients use (docs/remote-server.md
//! Section 3).
//!
//! The bridge, `status` and the desktop app read the active profile. `local` is built in: the
//! loopback daemon and its token file, exactly as before profiles existed. A remote profile names an
//! HTTPS URL, an optional CA bundle and a 0600 credential file. Every AI app is configured with
//! `supragnosis bridge` either way, so switching server changes nothing in any of them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const LOCAL: &str = "local";

/// Where the bridge sends requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local,
    Remote {
        name: String,
        url: String,
        ca: Option<PathBuf>,
        token_file: PathBuf,
    },
}

impl Target {
    pub fn name(&self) -> &str {
        match self {
            Target::Local => LOCAL,
            Target::Remote { name, .. } => name,
        }
    }
}

/// `~/.supragnosis/client.toml`. Holds no secret - credentials live in their own 0600 files.
#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClientFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub servers: BTreeMap<String, ServerEntry>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerEntry {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca: Option<String>,
    pub token_file: String,
}

pub fn client_path(home: &Path) -> PathBuf {
    home.join(".supragnosis/client.toml")
}

/// Where `server add` keeps a profile's credential.
pub fn token_path(home: &Path, name: &str) -> PathBuf {
    home.join(".supragnosis/servers").join(format!("{name}.token"))
}

/// The profile file, or an empty one when there is none. A file that does not parse is an error,
/// never "no profiles": that would quietly send every AI app back to the local daemon (P5).
pub fn load(home: &Path) -> Result<ClientFile, String> {
    let path = client_path(home);
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ClientFile::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn render(file: &ClientFile) -> String {
    format!(
        "# Which supragnosis server this machine's AI apps use (docs/remote-server.md).\n\
         # Managed by `supragnosis server`; credentials are in ~/.supragnosis/servers/.\n{}",
        toml::to_string_pretty(file).unwrap_or_default()
    )
}

/// A profile name: what `server use` takes and what a file name is built from.
pub fn valid_name(name: &str) -> Result<(), String> {
    if name == LOCAL {
        return Err("\"local\" is the built-in profile for this machine's daemon".into());
    }
    if name.is_empty()
        || name.len() > 64
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(format!("profile name {name:?}: use letters, digits, '-' or '_'"));
    }
    Ok(())
}

/// A remote server's MCP URL: HTTPS, or plain HTTP to loopback only (an SSH tunnel is the one
/// reason to want it). A URL with no path gets `/mcp`, the path every supragnosis server uses
/// (R8: a remote credential never crosses an unverified connection).
pub fn normalize_url(url: &str) -> Result<String, String> {
    let mut u =
        reqwest::Url::parse(url.trim()).map_err(|e| format!("{url:?} is not a URL: {e}"))?;
    let Some(host) = u.host_str() else {
        return Err(format!("{url:?} names no host"));
    };
    let loopback = host == "localhost"
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    match u.scheme() {
        "https" => {}
        "http" if loopback => {}
        "http" => {
            return Err(format!(
                "{url:?} is plain HTTP to another machine - the credential would cross the \
                 network unencrypted. Use https:// (the server's [server] listener terminates TLS)"
            ))
        }
        s => return Err(format!("{url:?}: scheme {s:?} is not http(s)")),
    }
    if u.path() == "/" || u.path().is_empty() {
        u.set_path("/mcp");
    }
    Ok(u.to_string())
}

/// The active target. The environment wins, for a container or CI job with no profile file:
/// `SUPRAGNOSIS_SERVER_URL` with `SUPRAGNOSIS_SERVER_TOKEN_FILE` (and optionally
/// `SUPRAGNOSIS_SERVER_CA`). Otherwise the file's `active`, and `local` when there is none.
pub fn active(home: &Path, env: impl Fn(&str) -> Option<String>) -> Result<Target, String> {
    let get = |k: &str| env(k).filter(|v| !v.trim().is_empty());
    if let Some(url) = get("SUPRAGNOSIS_SERVER_URL") {
        let token_file = get("SUPRAGNOSIS_SERVER_TOKEN_FILE").ok_or(
            "SUPRAGNOSIS_SERVER_URL is set without SUPRAGNOSIS_SERVER_TOKEN_FILE - the credential is \
             read from a file, never from the environment itself",
        )?;
        return Ok(Target::Remote {
            name: "env".into(),
            url: normalize_url(&url)?,
            ca: get("SUPRAGNOSIS_SERVER_CA").map(PathBuf::from),
            token_file: PathBuf::from(token_file),
        });
    }
    let file = load(home)?;
    match file.active.as_deref() {
        None | Some(LOCAL) => Ok(Target::Local),
        Some(name) => {
            let e = file.servers.get(name).ok_or_else(|| {
                format!(
                    "the active server profile {name:?} is not in {}",
                    client_path(home).display()
                )
            })?;
            Ok(Target::Remote {
                name: name.to_string(),
                url: e.url.clone(),
                ca: e.ca.as_ref().map(PathBuf::from),
                token_file: PathBuf::from(&e.token_file),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_server_is_reached_over_verified_tls_or_loopback() {
        assert_eq!(
            normalize_url("https://hub.example:7420").unwrap(),
            "https://hub.example:7420/mcp"
        );
        assert_eq!(
            normalize_url("https://hub.example/x/mcp").unwrap(),
            "https://hub.example/x/mcp"
        );
        assert_eq!(normalize_url("http://127.0.0.1:17420").unwrap(), "http://127.0.0.1:17420/mcp");
        assert!(normalize_url("http://hub.example:7420").unwrap_err().contains("unencrypted"));
        assert!(normalize_url("ftp://hub.example").is_err());
    }

    #[test]
    fn the_environment_names_a_server_only_with_a_credential_file() {
        let home = Path::new("/nonexistent-home");
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert_eq!(active(home, env(&[])).unwrap(), Target::Local, "no file, no env: local");
        assert!(active(home, env(&[("SUPRAGNOSIS_SERVER_URL", "https://hub.example")])).is_err());
        let t = active(
            home,
            env(&[
                ("SUPRAGNOSIS_SERVER_URL", "https://hub.example"),
                ("SUPRAGNOSIS_SERVER_TOKEN_FILE", "/run/secrets/supragnosis"),
            ]),
        )
        .unwrap();
        assert_eq!(
            t,
            Target::Remote {
                name: "env".into(),
                url: "https://hub.example/mcp".into(),
                ca: None,
                token_file: "/run/secrets/supragnosis".into()
            }
        );
    }

    #[test]
    fn the_profile_file_round_trips_and_names_are_checked() {
        let mut f = ClientFile { active: Some("home".into()), ..Default::default() };
        f.servers.insert(
            "home".into(),
            ServerEntry {
                url: "https://hub.example/mcp".into(),
                ca: None,
                token_file: "/t".into(),
            },
        );
        let back: ClientFile = toml::from_str(&render(&f)).unwrap();
        assert_eq!(back, f);
        assert!(valid_name("local").is_err());
        assert!(valid_name("../x").is_err());
        assert!(valid_name("home-lab_2").is_ok());
    }
}
