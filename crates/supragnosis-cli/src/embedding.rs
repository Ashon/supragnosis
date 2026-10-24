//! The embedder: building it, the store's record of the one that wrote its vectors, and the
//! `embed reset`, `embed rebuild` and `embed fetch` subcommands.

use super::*;

/// Selects the embedding provider from the SUPRAGNOSIS_EMBED value. Absence yields None (keyword
/// search); a model that is not there degrades later, loudly (semantic-retrieval.md Section 6).
/// The one refusal is a `SUPRAGNOSIS_EMBED_MODEL` that names no model (Principle 24).
pub(crate) fn build_embedder(kind: &str) -> Result<Option<Arc<dyn EmbeddingProvider>>> {
    Ok(match kind {
        "none" | "" => None,
        // Deterministic but non-semantic (lexical hashing) - a development/offline stand-in.
        "hashing" => {
            tracing::info!("embed=hashing (deterministic/lexical, for development)");
            Some(Arc::new(HashingEmbedder::default()))
        }
        "fastembed" => build_fastembed()?,
        other => {
            tracing::warn!(
                kind = other,
                "unknown SUPRAGNOSIS_EMBED - proceeding with keyword search"
            );
            None
        }
    })
}

/// The model `SUPRAGNOSIS_EMBED_MODEL` names, or the default (semantic-retrieval.md Section 5).
#[cfg(feature = "fastembed")]
pub(crate) fn embed_model() -> Result<supragnosis_embed::Model> {
    supragnosis_embed::Model::parse(&std::env::var("SUPRAGNOSIS_EMBED_MODEL").unwrap_or_default())
        .map_err(|e| anyhow::anyhow!(e))
}

/// The lazy provider: identity now, weights on first use from the cache, never a download (SR1,
/// SR2). A missing model is said once here, at start, and again by `status` for as long as it is.
#[cfg(feature = "fastembed")]
pub(crate) fn build_fastembed() -> Result<Option<Arc<dyn EmbeddingProvider>>> {
    use supragnosis_core::Readiness;
    use supragnosis_embed::{model_cache_dir, FastEmbedProvider};
    let model = embed_model()?;
    let cache = model_cache_dir();
    let provider = FastEmbedProvider::lazy(model, cache.clone());
    match provider.readiness() {
        Readiness::Ready => tracing::info!(
            model = model.name(),
            id = model.id(),
            cache = %cache.display(),
            "embed=fastembed - the model loads on first use"
        ),
        Readiness::Degraded(note) => tracing::warn!("{note}"),
    }
    Ok(Some(Arc::new(provider)))
}

#[cfg(not(feature = "fastembed"))]
pub(crate) fn build_fastembed() -> Result<Option<Arc<dyn EmbeddingProvider>>> {
    tracing::warn!("this build has no embedding model (built without the fastembed feature) - proceeding with keyword search");
    Ok(None)
}

/// Why a store is refused for the model it is opened with, and the two ways on
/// (semantic-retrieval.md Section 13, SR8).
pub(crate) fn embedder_refusal(recorded: &str, opened: &str) -> String {
    format!(
        "the store was embedded by '{recorded}' and this process opens it with '{opened}' - vectors \
         from two models share no space, so the store is refused (docs/semantic-retrieval.md Section \
         13). Either run `supragnosis embed rebuild` - in the app, Settings > This Mac > Rebuild \
         index - which stops the daemon, removes the old index and starts it again with '{opened}'; \
         or run without semantic search and leave the store as it is: SUPRAGNOSIS_EMBED=none, for \
         the login job `supragnosis service install --env SUPRAGNOSIS_EMBED=none`"
    )
}

/// The environment the login job runs the daemon with, when one is installed: launchd and systemd
/// start it with that and none of this shell's (semantic-retrieval.md Section 13). `None` when no
/// job is installed, or its environment cannot be read.
pub(crate) fn job_env() -> Option<std::collections::BTreeMap<String, String>> {
    if canonical_service_state() == "absent" {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        plist_env(&canonical_service_path()).ok()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let text = std::fs::read_to_string(canonical_service_path()).ok()?;
        let home = std::env::var("HOME").unwrap_or_default();
        lifecycle::parse_unit_env(&text, &home, &current_user().unwrap_or_default()).ok()
    }
}

/// A variable of the daemon's environment: the login job's when one is installed, this process's
/// otherwise; an empty value is no value, as `serve` reads it.
pub(crate) fn daemon_var(
    job: &Option<std::collections::BTreeMap<String, String>>,
    key: &str,
) -> Option<String> {
    match job {
        Some(vars) => vars.get(key).cloned(),
        None => std::env::var(key).ok(),
    }
    .filter(|v| !v.trim().is_empty())
}

/// The identity of the model a daemon with this environment opens its store with - what the store
/// records - without loading it; `None` when it runs without one.
pub(crate) fn embedder_id_for(get: &dyn Fn(&str) -> Option<String>) -> Result<Option<String>> {
    let kind = get("SUPRAGNOSIS_EMBED").unwrap_or_else(|| default_embed_kind().to_string());
    Ok(match kind.as_str() {
        "hashing" => Some(HashingEmbedder::default().id()),
        "fastembed" => fastembed_id(&get("SUPRAGNOSIS_EMBED_MODEL").unwrap_or_default())?,
        _ => None,
    })
}
#[cfg(feature = "fastembed")]
pub(crate) fn fastembed_id(model: &str) -> Result<Option<String>> {
    let model = supragnosis_embed::Model::parse(model).map_err(|e| anyhow::anyhow!(e))?;
    Ok(Some(model.id().to_string()))
}
#[cfg(not(feature = "fastembed"))]
pub(crate) fn fastembed_id(_model: &str) -> Result<Option<String>> {
    Ok(None)
}

/// The model the store at `store` recorded and the one the daemon would open it with, when they
/// differ and nothing holds the store: why a daemon cannot start, which `status` names (SR8).
pub(crate) fn embedder_conflict(
    store: &std::path::Path,
    daemon: Option<String>,
) -> Option<(String, String)> {
    if supragnosis_store::redb_in_use(store) {
        return None;
    }
    let recorded = supragnosis_store::recorded_embedder(store).ok().flatten()?;
    let daemon = daemon?;
    (recorded != daemon).then_some((recorded, daemon))
}

/// `status`'s line for a store the daemon cannot open (SR8).
pub(crate) fn embedder_conflict_line(recorded: &str, daemon: &str) -> String {
    format!(
        "  store   the daemon cannot open it: it was embedded by '{recorded}' and the daemon runs \
         '{daemon}'. `supragnosis embed rebuild` - in the app, Settings > This Mac > Rebuild index - \
         rebuilds it with '{daemon}'; or `supragnosis service install --env SUPRAGNOSIS_EMBED=none` \
         runs without semantic search"
    )
}

/// `supragnosis embed reset` (semantic-retrieval.md Section 13, SR9): on a store no process holds,
/// removes every stored vector and the model the store recorded, so the next start records the
/// model the daemon runs and its backfill embeds every row with it. The store is the daemon's: the
/// one its login job's environment names, when one is installed.
pub(crate) fn embed_reset() -> Result<()> {
    let job = job_env();
    let get = |k: &str| daemon_var(&job, k);
    let Some(path) = daemon_store_path(get) else {
        println!("The daemon's store is in memory, so there is nothing to reset.");
        return Ok(());
    };
    if !path.exists() {
        println!("There is no store at {} - nothing to reset.", path.display());
        return Ok(());
    }
    if supragnosis_store::redb_in_use(&path) {
        anyhow::bail!(
            "the store at {} is open in another process - the daemon, or an AI app's stdio server - \
             so nothing was changed. Stop it (`supragnosis stop`) and run this again, or run \
             `supragnosis embed rebuild`, which stops and starts a daemon this CLI manages",
            path.display()
        );
    }
    let store = RedbStore::open(&path)
        .with_context(|| format!("failed to open redb store at {}", path.display()))?;
    let r = store.reset_embeddings()?;
    drop(store);
    match &r.model {
        None if r.observations + r.entities == 0 => {
            println!("The store holds no vectors and records no model - nothing to reset.")
        }
        Some(model) if r.observations + r.entities == 0 => println!(
            "The store held no vectors; removed its record of '{model}'. The log and the graph are \
             as they were."
        ),
        model => println!(
            "Removed {} vectors ({} for observations, {} for entities) made by {}, and the store's \
             record of that model. The log and the graph are as they were.",
            r.observations + r.entities,
            r.observations,
            r.entities,
            model.as_deref().map_or("no recorded model".to_string(), |m| format!("'{m}'"))
        ),
    }
    match embedder_id_for(&get)? {
        Some(id) => println!(
            "The next start records '{id}' and embeds every observation and entity with it; \
             `supragnosis status` shows it while it runs."
        ),
        None => println!(
            "The daemon runs without a model (SUPRAGNOSIS_EMBED={}), so the store stays without \
             vectors and search matches words. `supragnosis service install --env \
             SUPRAGNOSIS_EMBED=fastembed` gives the login job the model.",
            get("SUPRAGNOSIS_EMBED").unwrap_or_else(|| default_embed_kind().to_string())
        ),
    }
    Ok(())
}

/// What `supragnosis embed rebuild` does for the daemon it finds (semantic-retrieval.md Section 13,
/// SR10). It stops only a daemon this CLI manages for this home - a `supragnosis start` daemon,
/// whose pidfile is this home's, or the login job whose file is in this home - and refuses every
/// other holder with nothing changed: a login job is per user, not per home, so a job loaded from
/// another home's file is not this store's to stop.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RebuildPlan {
    /// Nothing runs: reset the store, and start the login job when this home installs one.
    ResetOnly { start_job: bool },
    /// Stop the daemon, reset the store, start the daemon again.
    StopResetStart,
    /// Refuse, changing nothing, with the reason and the way on.
    Refuse(String),
}

pub(crate) fn rebuild_plan(s: &lifecycle::Situation, service_state: &str) -> RebuildPlan {
    use lifecycle::{LabelKind, Manager, Situation};
    let installed = service_state != "absent";
    match s {
        Situation::Stopped => RebuildPlan::ResetOnly { start_job: installed },
        Situation::One(Manager::Pidfile { .. }) => RebuildPlan::StopResetStart,
        Situation::One(Manager::Launchd(j) | Manager::Systemd(j))
            if j.kind == LabelKind::Canonical && installed =>
        {
            RebuildPlan::StopResetStart
        }
        Situation::One(m) => RebuildPlan::Refuse(format!(
            "the daemon runs under {}, which this command does not stop and start for this home - \
             nothing was changed. Stop it there, run `supragnosis embed reset`, and start it again",
            m.describe()
        )),
        Situation::Conflict(ms) => RebuildPlan::Refuse(format!(
            "{} - nothing was changed",
            lifecycle::conflict_message(ms)
        )),
        Situation::Unrecognized => RebuildPlan::Refuse(
            "the store is held by a process no manager runs - an AI app's stdio server, or a \
             `supragnosis serve` in a terminal - so nothing was changed. Stop it, then run \
             `supragnosis embed reset`"
                .to_string(),
        ),
    }
}

/// `supragnosis embed rebuild` (semantic-retrieval.md Section 13, SR10): the search index made again
/// from the log by the model the daemon runs. It stops the daemon this CLI manages, resets the store
/// (SR9) and starts the daemon again, whose backfill embeds every row; with no daemon running it
/// resets the store and starts the login job when one is installed.
pub(crate) fn embed_rebuild() -> Result<()> {
    clear_stale_pidfile();
    let plan = rebuild_plan(&lifecycle::classify(&observe()), canonical_service_state());
    let start = match plan {
        RebuildPlan::Refuse(why) => anyhow::bail!(why),
        RebuildPlan::ResetOnly { start_job } => start_job,
        RebuildPlan::StopResetStart => {
            stop()?;
            // The stopped process lets go of the store as it exits.
            let job = job_env();
            if let Some(path) = daemon_store_path(|k| daemon_var(&job, k)) {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
                while supragnosis_store::redb_in_use(&path) {
                    if std::time::Instant::now() > deadline {
                        anyhow::bail!(
                            "the daemon stopped but the store at {} is still held after 15 seconds - \
                             `supragnosis status` says what holds it",
                            path.display()
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            true
        }
    };
    embed_reset()?;
    if start {
        restart(resolve(RunArgs::default(), true))?;
    } else {
        println!("No daemon runs here; the next `supragnosis start` embeds the store.");
    }
    Ok(())
}

/// The model the daemon's store recorded and the one it runs, when the daemon does not answer
/// because they differ (SR8); what `status` and setup's server step say.
pub(crate) fn daemon_embedder_conflict() -> Option<(String, String)> {
    let job = job_env();
    let get = |k: &str| daemon_var(&job, k);
    daemon_store_path(get).and_then(|p| embedder_conflict(&p, embedder_id_for(&get).ok().flatten()))
}

/// Setup's server step for a daemon that cannot open its store (onboarding.md Section 4.1).
pub(crate) fn embedder_conflict_attention(recorded: &str, daemon: &str) -> String {
    format!(
        "The daemon cannot start: its store was indexed by '{recorded}' and it runs '{daemon}'. \
         Rebuild index in Settings > This Mac, or `supragnosis embed rebuild`, rebuilds the index \
         with '{daemon}'."
    )
}

/// `supragnosis embed fetch` (semantic-retrieval.md Section 6): the one download, by the person's
/// or the installer's command. Idempotent, and says what it found or fetched. With `events`, the
/// progress bar gives way to one JSON line about twice a second with the bytes on disk, and the last
/// line says whether the model is complete (docs/onboarding.md Section 4.3).
#[cfg(feature = "fastembed")]
pub(crate) fn embed_fetch(progress: bool, events: bool) -> Result<()> {
    use supragnosis_embed::{model_cache_dir, FastEmbedProvider};
    let model = embed_model()?;
    let cache = model_cache_dir();
    if events {
        return embed_fetch_events(model, &cache);
    }
    let had = model.present(&cache);
    if !had {
        println!(
            "fetching the embedding model {} ({} MB, once) into {}",
            model.name(),
            model.download_mb(),
            cache.display()
        );
    }
    let bytes = FastEmbedProvider::fetch(model, &cache, progress && !had)
        .map_err(|e| anyhow::anyhow!("fetching the embedding model {}: {e}", model.name()))?;
    println!(
        "{} the embedding model {} ({} MB on disk) in {}",
        if had { "found" } else { "fetched" },
        model.name(),
        bytes >> 20,
        cache.display()
    );
    Ok(())
}

/// `embed fetch --events`: the fetch on a thread, and the model's directory measured beside it. A
/// download in progress is `working`; only a complete model is `done` (O5).
#[cfg(feature = "fastembed")]
pub(crate) fn embed_fetch_events(
    model: supragnosis_embed::Model,
    cache: &std::path::Path,
) -> Result<()> {
    use setup::{Event, State};
    use supragnosis_embed::FastEmbedProvider;
    let emit = |e: &Event| println!("{}", serde_json::to_string(e).unwrap_or_default());
    println!("{}", serde_json::json!({ "schema": setup::EVENTS_SCHEMA }));
    let mb = model.download_mb();
    if model.present(cache) {
        emit(&Event::new(
            "model",
            None,
            State::Done,
            "Search by meaning is on: the model is here.",
        ));
        return Ok(());
    }
    emit(&Event::new(
        "model",
        None,
        State::Working,
        format!(
            "Downloading the search model {} ({mb} MB, once) into {}.",
            model.name(),
            cache.display()
        ),
    ));
    let (tx, rx) = std::sync::mpsc::channel();
    let to = cache.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(FastEmbedProvider::fetch(model, &to, false));
    });
    let result = loop {
        match rx.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(r) => break r,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => emit(&Event::new(
                "model",
                None,
                State::Working,
                setup::progress_say(model.bytes_on_disk(cache), mb),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break Err(supragnosis_core::EmbedError::Provider("the download stopped".into()))
            }
        }
    };
    match result {
        Ok(_) if model.present(cache) => {
            emit(&Event::new(
                "model",
                None,
                State::Done,
                "Search by meaning is on: the model is here.",
            ));
            Ok(())
        }
        Ok(_) => {
            let say = format!("The download ended, but the model in {} is not complete. Run setup again to fetch what is missing.", cache.display());
            emit(&Event::new("model", None, State::Failed, say.clone()));
            anyhow::bail!(say)
        }
        Err(e) => {
            let say = format!("The download failed ({e}). Search matches words until it is here; run setup again when online.");
            emit(&Event::new("model", None, State::Failed, say.clone()));
            anyhow::bail!(say)
        }
    }
}

#[cfg(not(feature = "fastembed"))]
pub(crate) fn embed_fetch(_progress: bool, _events: bool) -> Result<()> {
    anyhow::bail!(
        "this build has no embedding model (built without the fastembed feature); retrieval is keyword only"
    )
}
