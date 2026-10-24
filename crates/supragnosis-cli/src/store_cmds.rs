//! The one-shot subcommands that open the store or the node's identity: `identity`,
//! `migrate`, `rekey-workspace`, `reproject` and `sync`.

use super::*;

/// `supragnosis identity` - prints the node's federation identity (generating the keypair on first
/// use); --hash-token prints what a server allowlist entry stores for a peer's bearer token.
pub(crate) fn identity_cmd(a: IdentityArgs) -> Result<()> {
    init_tracing();
    let id = fed::load_or_create_identity()?;
    println!("node_id:     {}", id.node_id());
    println!("public_key:  {}", id.public_key_hex());
    if let Some(tok) = a.hash_token {
        println!("bearer_hash: {}", blake3::hash(tok.as_bytes()).to_hex());
    }
    Ok(())
}

/// `supragnosis migrate` - one-shot legacy-id migration (docs/federation.md: a stored id that
/// predates the current content-address formula cannot verify remotely; the row is re-created under
/// the current id with lineage back to the legacy row). Re-materializes afterwards.
pub(crate) fn migrate_cmd(a: WorkspaceArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let ws = a.workspace.unwrap_or_else(|| cfg.workspace.clone());
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let m = engine.migrate_legacy_ids(&ws)?;
        let r = engine.reproject(Some(&ws))?;
        println!(
            "migrated {} legacy-id observation(s); reprojected {}: {} observations -> {} entities, {} relations",
            m.migrated, ws, r.observations, r.entities, r.relations
        );
        if m.skipped_proposal_events > 0 {
            println!(
                "left {} legacy-id proposal-event row(s) as they are - re-keying one would orphan its verdicts",
                m.skipped_proposal_events
            );
        }
        if m.promotions_left_behind > 0 {
            println!(
                "{} migrated row(s) had a merged promotion, which names the legacy id - promote them again",
                m.promotions_left_behind
            );
        }
        anyhow::Ok(())
    })
}

/// `supragnosis rekey-workspace` - re-create a workspace's knowledge under another name.
///
/// The workspace is inside the content address, so this cannot be a move: the re-keyed rows are new
/// observations and the originals stay (Principle 3). It is not a re-ingest either - every
/// attestation is copied verbatim, so the original observation times and authors survive, which
/// pushing the text back through `observe` would destroy. The store is single-process: stop the
/// daemon first.
pub(crate) fn rekey_workspace_cmd(a: RekeyArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let rep = engine.rekey_workspace(&a.from, &a.to, a.dry_run)?;
        let verb = if a.dry_run { "would re-key" } else { "re-keyed" };
        println!(
            "{verb} {} observation(s) from {} to {} ({} already there, {} proposal-event row(s) left behind)",
            rep.moved, a.from, a.to, rep.already, rep.skipped_proposal_events
        );
        if rep.promotions_left_behind > 0 {
            println!(
                "{} of them had a merged promotion, which stays in {} - promote them again in {} if they should be",
                rep.promotions_left_behind, a.from, a.to
            );
        }
        if a.dry_run {
            println!("dry run - nothing was written");
        } else if rep.moved > 0 {
            let r = engine.reproject(Some(&a.to))?;
            println!(
                "reprojected {}: {} observations -> {} entities, {} relations",
                a.to, r.observations, r.entities, r.relations
            );
        }
        anyhow::Ok(())
    })
}

/// `supragnosis reproject` - one-shot re-materialization (HLC-ordered replay, Prop C). For a node
/// whose log advanced without projection (e.g. a hub that received pushes before the reproject hook
/// existed). The store is single-process: stop the daemon first.
pub(crate) fn reproject_cmd(a: WorkspaceArgs) -> Result<()> {
    init_tracing();
    let cfg = resolve(RunArgs::default(), false);
    let ws = a.workspace.unwrap_or_else(|| cfg.workspace.clone());
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let r = engine.reproject(Some(&ws))?;
        println!(
            "reprojected {}: {} observations -> {} entities, {} relations",
            ws, r.observations, r.entities, r.relations
        );
        anyhow::Ok(())
    })
}

/// `supragnosis sync` - one full sync round (push surplus, pull deficit, re-materialize) against
/// every configured server. Requires supragnosis.toml with [sync] servers + auth_token. The store
/// is single-process (RocksDB lock): with a running daemon, use the sync_* MCP tools instead.
pub(crate) fn sync_cmd(a: WorkspaceArgs) -> Result<()> {
    init_tracing();
    // A running daemon holds the store, so it runs the round itself, over its shared workspaces
    // (docs/pairing.md Section 4.5); only with none running does this process sync on its own.
    if let Some(round) = hub::call("POST", "/sync")? {
        if let Some(e) = round
            .get("error")
            .and_then(|e| e.as_str())
            .filter(|_| round.get("at").is_none())
        {
            anyhow::bail!("{e}");
        }
        let shared: Vec<&str> = round["workspaces"]
            .as_array()
            .map(|w| w.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        if round["servers"] == 0 {
            println!("nothing to sync: [sync] names no server");
            return Ok(());
        }
        if shared.is_empty() {
            println!("nothing to sync: [sync] shares no workspace (share_workspaces is empty)");
            return Ok(());
        }
        println!(
            "the running daemon synced {}: pushed {}, applied {}",
            shared.join(", "),
            round["pushed"],
            round["applied"]
        );
        if round["ok"] == false {
            anyhow::bail!(
                "the round failed: {}",
                round["error"].as_str().unwrap_or("no reason given")
            );
        }
        return Ok(());
    }
    let cfg = resolve(RunArgs::default(), false);
    let fc = fed::load()?.ok_or_else(|| {
        anyhow::anyhow!(
            "no federation config at {} - create it with a [sync] section (servers, auth_token, \
             share_workspaces). See docs/federation.md Section 9",
            fed::config_path().display()
        )
    })?;
    let (links, notes) = fc.sync.links();
    for n in &notes {
        tracing::error!("federation configuration: {n}");
    }
    anyhow::ensure!(
        !links.is_empty(),
        "[sync] names no server - nothing to sync against. Add [[sync.server]] entries with a url \
         and an auth_token each"
    );
    let identity = fed::load_or_create_identity()?;
    let node = supragnosis_sync::SyncNode::new(identity)
        .with_seq_mark(fed::fed_base_dir().join("node.seq"));
    // A node a join set up syncs what it shares and what its join recorded, as its daemon's round
    // does, and no other workspace (docs/pairing.md Section 4.5, PA12); any other node syncs the
    // one workspace named, or its default.
    let share = &fc.sync.share_workspaces;
    let workspaces: Vec<String> = match (a.workspace, {
        let (receive, note) = join::receive_list(&join::Paths::default_paths());
        if let Some(note) = note {
            eprintln!("{note}");
        }
        (receive, ())
    }) {
        (Some(ws), (Some(receive), _)) => {
            anyhow::ensure!(
                share.contains(&ws) || receive.contains(&ws),
                "this node syncs only the workspaces it shares and the ones its join recorded \
                 ({}); {ws} is neither",
                share.iter().chain(&receive).cloned().collect::<Vec<_>>().join(", ")
            );
            vec![ws]
        }
        (Some(ws), _) => vec![ws],
        (None, (Some(receive), _)) => {
            let mut all = share.clone();
            all.extend(receive.into_iter().filter(|w| !share.contains(w)));
            all
        }
        (None, (None, _)) => vec![cfg.workspace.clone()],
    };
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(async {
        let engine = build_engine(&cfg, None)?;
        let store = engine.store();
        let mut keys = trusted_origin_keys(&fc.sync);
        keys.insert(node.node_id().to_string(), node.public_key_hex());
        let (vouched, vouched_note) = join::vouched_keys(&join::Paths::default_paths(), &mut keys);
        if let Some(note) = vouched_note {
            eprintln!("{note}");
        }
        for ws in &workspaces {
            for link in &links {
                let server = &link.url;
                let client = supragnosis_sync::http::SyncClient::for_link(link)?
                    .with_vouched(vouched.clone());
                let s = client.sync_workspace(&store, &node, ws, share, &keys).await?;
                println!(
                    "{server} {ws}: pushed {} pulled {} (rejected: by server {}, locally {})",
                    s.pushed, s.pulled, s.rejected_by_server, s.rejected_locally
                );
            }
            let r = engine.reproject(Some(ws))?;
            println!(
                "reprojected {}: {} observations -> {} entities, {} relations",
                ws, r.observations, r.entities, r.relations
            );
        }
        anyhow::Ok(())
    })
}
