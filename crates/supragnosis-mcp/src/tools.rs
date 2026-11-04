//! The tools: the one `#[tool_router]` impl block, a method per tool.

use super::*;

#[tool_router(vis = "pub(crate)")]
impl SupragnosisServer {
    #[tool(
        description = "Ingest a knowledge fragment - how anything new enters the graph. Stores it as an immutable observation (the source of truth: superseded later, never edited or deleted) and links the entities and relations you extracted alongside it into the ontology. Use it as facts, decisions and the reasons behind them come up; to reshape knowledge that is already stored - merging duplicates, changing trust, retracting - use `propose` instead. Returns the observation id, the linked entity/relation ids and the workspace written to; a write that makes a new workspace adds a note naming the ones that hold knowledge."
    )]
    async fn observe(&self, Parameters(req): Parameters<ObserveRequest>) -> String {
        if let Some(why) = self.all_is_no_place("observe", &req.workspace) {
            return err_json(&why);
        }
        // Carried on the event so the viewer knows the scope.
        let workspace = req.workspace.clone();
        let input = ObserveInput {
            content: req.content,
            workspace: Some(req.workspace),
            source_ref: req.source_ref,
            confidence: req.confidence,
            on_behalf_of: req.on_behalf_of,
            derived_from: req.derived_from.iter().map(|d| lineage_id(d)).collect(),
            entities: req
                .entities
                .into_iter()
                .map(|e| EngineEntityInput {
                    name: e.name,
                    kind: e.kind,
                    description: e.description,
                })
                .collect(),
            relations: req
                .relations
                .into_iter()
                .map(|r| EngineRelationInput {
                    from: r.from,
                    kind: r.kind,
                    to: r.to,
                    description: r.description,
                    valid_from: r.valid_from,
                    valid_to: r.valid_to,
                })
                .collect(),
        };
        // Run the blocking store call on spawn_blocking so concurrent HTTP requests do not starve
        // the tokio workers (harmless with a single stdio client, but essential for a daemon with multiple connections).
        // Whether this write makes a new workspace, asked before it (design-system.md Section 29.20).
        let new_ws = matches!(self.engine.holds_observations(&workspace), Ok(false));
        let engine = self.engine.clone();
        let app = writing_app();
        let as_app = app.clone();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(as_app.as_deref(), || engine.observe(input))
        })
        .await
        {
            Ok(Ok(out)) => {
                // UI observability: publish the ingest activity (viewer live log + new-node pulse).
                self.engine.emit_from(
                    app.as_deref(),
                    Event::Observe {
                        observation: out.observation_id.clone(),
                        entities: out.entities.clone(),
                        relations: out.relations.len(),
                        workspace: workspace.clone(),
                    },
                );
                let mut answer = serde_json::to_value(&out).unwrap_or(serde_json::Value::Null);
                answer["workspace"] = workspace.clone().into();
                if new_ws {
                    if let Some(note) = self.new_workspace_note(&workspace) {
                        answer["note"] = note.into();
                    }
                }
                answer.to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(description = "Look up an entity and its relations/provenance by entity id.")]
    async fn get_entity(&self, Parameters(req): Parameters<GetEntityRequest>) -> String {
        let id = req.id.clone();
        let engine = self.engine.clone();
        let looked_up = tokio::task::spawn_blocking(move || {
            let view = engine.get_entity(&id)?;
            // An id that names no entity may name an observation: a search_knowledge hit of kind
            // observation, passed here. The point read says which, and its failure is the store's,
            // answered as one (Principle 5).
            let observation = view.is_none() && engine.get_observation(&id)?.is_some();
            Ok::<_, supragnosis_core::StoreError>((view, observation))
        })
        .await;
        match looked_up {
            Ok(Ok((Some(view), _))) => {
                self.engine.emit_from(
                    writing_app().as_deref(),
                    Event::GetEntity {
                        id: req.id.clone(),
                        name: Some(view.entity.canonical_name.clone()),
                        found: true,
                    },
                );
                to_json(&view)
            }
            // Open-world assumption (Principle 5): absence is not falsehood but unknown.
            // We do not return "not found" as an error, so the LLM does not misread absence as negation.
            Ok(Ok((None, observation))) => self.unknown_entity(&req.id, observation),
            // A backend failure differs from absence (Principle 5) - surfaced as an explicit error so it is not misread as "nothing there".
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Search knowledge (entities/observations). The response mode reports the surface actually used: hybrid (semantic+keyword) or keyword (keyword-only degrade - an empty result in this mode may be a recall failure). score is for rank comparison only - its scale differs per mode, so the absolute value is not a confidence."
    )]
    async fn search_knowledge(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        // The keyword rule takes an empty query for a phrase every text contains, so a blank one
        // would answer the first rows of the store as if they matched.
        if req.query.trim().is_empty() {
            return err_json(
                "query is empty - search for words; to see what a workspace holds, use workspace_map",
            );
        }
        let scope = req.scope.as_deref().unwrap_or("local");
        // Federated retrieval (scope remote/both): query the configured sync servers' ontologies -
        // local state can lag the remote store, so a local miss may just mean "not synced yet".
        let mut remote_results: Vec<serde_json::Value> = Vec::new();
        if scope == "remote" || scope == "both" {
            match &self.sync {
                None => remote_results.push(serde_json::json!({
                    "error": "no sync servers configured on this node (supragnosis.toml [sync])"
                })),
                Some(ctx) => {
                    let ws_name = req
                        .workspace
                        .clone()
                        .unwrap_or_else(|| self.engine.default_workspace().to_string());
                    // A query names what it is about, so it leaves only for a workspace this node
                    // shares, as a push does (sync-correctness.md Section 9). The local half below
                    // still answers.
                    if !ctx.share_workspaces.iter().any(|w| w == &ws_name) {
                        remote_results.push(serde_json::json!({
                            "skipped_workspace": ws_name,
                            "why": "not in [sync] share_workspaces - a query does not leave for a \
                                    workspace this node does not share. Add it there to search \
                                    hosts for it",
                        }));
                    } else {
                        // Ask only hosts that admit this workspace, and name the ones left out. Without
                        // the naming a narrowed recall is indistinguishable from an empty world, which is
                        // the reading P5 exists to prevent (N5); without the narrowing every non-
                        // admitting host answers 403 and the misses drown in denials.
                        let routed = supragnosis_sync::route(&ctx.servers, &ctx.surfaces, &ws_name);
                        if !routed.skipped.is_empty() {
                            remote_results.push(serde_json::json!({
                                "skipped": routed.skipped,
                                "why": "these hosts do not admit this workspace - not consulted, not empty",
                            }));
                        }
                        for link in ctx.servers.iter().filter(|l| routed.consult.contains(&l.url)) {
                            let server = &link.url;
                            let client = match supragnosis_sync::http::SyncClient::for_link(link) {
                                Ok(c) => c,
                                Err(e) => {
                                    remote_results.push(
                                        serde_json::json!({"server": server, "error": e.said()}),
                                    );
                                    continue;
                                }
                            };
                            match client.search(&ws_name, &req.query, req.limit.unwrap_or(20)).await
                            {
                                Ok(resp) => {
                                    self.engine.emit(Event::Sync {
                                        direction: "search".into(),
                                        peer: server.clone(),
                                        workspace: ws_name.clone(),
                                        count: resp.hits.len(),
                                    });
                                    remote_results.push(serde_json::json!({
                                        "server": server,
                                        "mode": resp.mode,
                                        "hits": resp.hits,
                                        "note": "remote retrieval surface - sync_pull this workspace to materialize the hits locally before traverse/get_entity",
                                    }));
                                }
                                Err(e) => remote_results
                                    .push(serde_json::json!({"server": server, "error": e.said()})),
                            }
                        }
                    }
                }
            }
        }
        if scope == "remote" {
            return to_json(&serde_json::json!({ "scope": "remote", "remote": remote_results }));
        }
        let query = req.query.clone();
        // `*` is every workspace here, as an omitted one is (design-system.md Section 29.15).
        let ws = req.workspace.clone().filter(|w| !is_all(w));
        let limit = req.limit.unwrap_or(20);
        let engine = self.engine.clone();
        let searched =
            tokio::task::spawn_blocking(move || engine.search(&query, ws.as_deref(), limit)).await;
        let searched = match searched {
            Ok(r) => r,
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        match searched {
            Ok(out) => {
                // UI observability: publish the search activity (viewer log + hit-node highlight).
                self.engine.emit_from(
                    writing_app().as_deref(),
                    Event::Search {
                        query: req.query.clone(),
                        workspace: req.workspace.clone(),
                        hits: out.hits.len(),
                        nodes: out.hits.iter().map(|h| h.id.clone()).collect(),
                        mode: match out.mode {
                            SearchMode::Hybrid => "hybrid",
                            SearchMode::Keyword => "keyword",
                        }
                        .to_string(),
                    },
                );
                let mut resp =
                    serde_json::json!({ "mode": out.mode, "hits": self.named_hits(&out.hits) });
                if !remote_results.is_empty() {
                    resp["remote"] = serde_json::Value::Array(remote_results);
                }
                // Open-world assumption (Principle 5): zero hits is unknown, not a negation. For a keyword
                // degrade, we also signal that zero hits is more likely a recall failure, to aid self-correction (Principle 21).
                if out.hits.is_empty() {
                    resp["note"] = serde_json::Value::String(match out.mode {
                        supragnosis_engine::SearchMode::Hybrid => {
                            "no hits - absence is unknown, not a negation (open-world). \
                             The knowledge may not be ingested or phrased differently; \
                             try other terms or traverse from a related entity"
                                .into()
                        }
                        supragnosis_engine::SearchMode::Keyword => {
                            "no hits under keyword-only degrade (semantic retrieval UNAVAILABLE) \
                             - a miss here is weak evidence of absence, not a negation. Every \
                             term must appear (in any order, each as a substring), so retry with \
                             fewer or shorter terms the knowledge would contain"
                                .into()
                        }
                    });
                }
                resp.to_string()
            }
            Err(e) => store_failure_json(&e),
        }
    }

    #[tool(
        description = "Traverse the graph from an entity, following relation direction (from->to). Returns the entities reachable within the maximum hops (max_depth), along with their shortest distance. Reachability runs THROUGH a relation endpoint that no entity row describes yet (partial ingest), but such an endpoint is not listed - so a hit can sit deeper than any listed neighbor. That gap is unknown, not a missing edge."
    )]
    async fn traverse(&self, Parameters(req): Parameters<TraverseRequest>) -> String {
        let id = req.id.clone();
        let max_depth = req.max_depth.unwrap_or(3);
        let limit = req.limit.unwrap_or(100);
        let engine = self.engine.clone();
        let traversed =
            tokio::task::spawn_blocking(move || engine.traverse(&id, max_depth, limit)).await;
        let hits = match traversed {
            Ok(Ok(h)) => h,
            Ok(Err(e)) => return store_failure_json(&e),
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // UI observability: publish the traverse activity (highlight the start + reached nodes).
        self.engine.emit_from(
            writing_app().as_deref(),
            Event::Traverse {
                start: req.id.clone(),
                reached: hits.iter().map(|h| h.id.clone()).collect(),
            },
        );
        let mut resp = serde_json::json!({ "hits": hits });
        // Distinguish the cause of zero hits (Principles 5/21): a missing start entity (unknown) and
        // "exists but has no outgoing relations" are situations the LLM must correct for differently.
        // The confirming get_entity is also blocking, so offload it on spawn_blocking.
        if hits.is_empty() {
            let id2 = req.id.clone();
            let engine2 = self.engine.clone();
            let exists = tokio::task::spawn_blocking(move || engine2.get_entity(&id2)).await;
            resp["note"] = serde_json::Value::String(match exists {
                Ok(Ok(Some(_))) => "start entity exists but reached no entities - it has \
                                    no outgoing relations within max_depth (absence of \
                                    edges is unknown, not a negation)"
                    .into(),
                Ok(Ok(None)) => "start entity id not found - unknown, not a negation \
                                 (open-world). Find the id via search_knowledge first"
                    .into(),
                _ => "empty result; start entity could not be checked due to a \
                      storage failure - do not conclude absence"
                    .into(),
            });
        }
        resp.to_string()
    }

    #[tool(
        description = "Survey a workspace's main co-occurrence contexts (hyperedges - the set of entities asserted together in a single observation). For cold-start orientation: before searching, grasp by name 'what is here and in what clusters'. Clusters are sorted by size (number of co-occurring entities) and sources is the number of supporting observations. This is a directional signal, not an asserted relation - confirm actual relations/details with search_knowledge/get_entity. To solidify a recurring context into first-class structure, observe a group entity (e.g. type Context) with member_of relations from each member and derived_from listing the co-asserting observation ids - the assertion then carries provenance and trust like any other edge."
    )]
    async fn workspace_map(&self, Parameters(req): Parameters<WorkspaceMapRequest>) -> String {
        // Omitted, '*', 'all' or '' reads every workspace (None), as search_knowledge does.
        let ws_arg = req.workspace.filter(|w| !is_all(w));
        let limit = req.limit.unwrap_or(20);
        // The minimum size cannot go below 2 (size<2 is not a hyperedge).
        let min_size = req.min_size.unwrap_or(2).max(2);
        let engine = self.engine.clone();
        let ws_call = ws_arg.clone();
        let mapped =
            tokio::task::spawn_blocking(move || engine.hypergraph(ws_call.as_deref())).await;
        let hg = match mapped {
            Ok(Ok(hg)) => hg,
            Ok(Err(e)) => return store_failure_json(&e),
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // A name-centric, readable summary (Principle 21). Hyperedges are already sorted (size desc, id asc).
        let qualifying = hg.hyperedges.iter().filter(|h| h.size >= min_size).count();
        let clusters: Vec<serde_json::Value> = hg
            .hyperedges
            .iter()
            .filter(|h| h.size >= min_size)
            .take(limit)
            .map(|h| {
                // Names for reading, ids for the next call (P2/P14): the hyperedge id names the
                // cluster stably, and the member ids are what get_entity takes.
                serde_json::json!({
                    "id": h.id,
                    "concepts": h.member_names,
                    "members": h.members,
                    "size": h.size,
                    "sources": h.sources,
                    "trust_tier": h.trust_tier,
                })
            })
            .collect();
        let shown = clusters.len();
        // ws_arg moves into the response below; keep the scope for the empty-result diagnosis.
        let scope = ws_arg.clone();
        let mut resp = serde_json::json!({
            "workspace": ws_arg,
            "clusters": clusters,
            "stats": {
                "node_count": hg.stats.node_count,
                "hyperedge_count": hg.stats.hyperedge_count,
                "max_size": hg.stats.max_size,
                "shown": shown,
                "matched": qualifying,
            },
        });
        // Do not silence truncation (no silent caps) + zero hits is absence != negation (Principle 5).
        if qualifying > shown {
            resp["note"] = serde_json::Value::String(format!(
                "showing top {shown} of {qualifying} clusters (by size). raise limit or lower \
                 min_size to see more. clusters are co-occurrence contexts (entities asserted \
                 together), not asserted relations - drill in with get_entity by member id, or \
                 search_knowledge by concept name"
            ));
        } else if shown == 0 && hg.stats.node_count == 0 {
            // An empty named workspace is usually a wrong name, not an absent ontology. A reader who
            // does not already know the names has no next move, so name them here - the dead end
            // corrects itself within the same call rather than requiring a lucky retry with '*'.
            let engine = self.engine.clone();
            let listed = tokio::task::spawn_blocking(move || engine.workspaces()).await;
            let elsewhere: Vec<String> = match listed {
                Ok(Ok(all)) => {
                    all.into_iter().filter(|w| Some(w.as_str()) != scope.as_deref()).collect()
                }
                // Enumeration is a diagnostic nicety, never a reason to fail the survey.
                _ => Vec::new(),
            };
            let where_ = match &scope {
                Some(ws) => format!("workspace '{ws}'"),
                None => "this node".to_string(),
            };
            resp["note"] = serde_json::Value::String(if elsewhere.is_empty() {
                format!(
                    "{where_} holds no entities - nothing has been observed into it yet. absence is \
                     unknown, not a negation (open-world). observe to populate it"
                )
            } else {
                format!(
                    "{where_} holds no entities, but this node does hold knowledge - a scope miss, not \
                     an absent ontology. Knowledge exists in: {}. Re-run scoped to one of those, or \
                     without a workspace to survey all",
                    elsewhere.join(", ")
                )
            });
            resp["knowledge_in_workspaces"] = serde_json::json!(elsewhere);
        } else if shown == 0 {
            resp["note"] = serde_json::Value::String(
                "no co-occurrence clusters at this min_size - absence is unknown, not a negation \
                 (open-world). Lower min_size, widen workspace ('*'), or the workspace may be \
                 sparsely linked (entities observed alone). observe more, or use search_knowledge"
                    .into(),
            );
        }
        resp.to_string()
    }

    #[tool(
        description = "Define ontology types (T-Box) for a workspace: give an entity type or relation type a name and a natural-language definition of what it means. Use this to record the vocabulary of the ontology so a type is not just a bare label but has a stated meaning (e.g. entity type 'Driver' = 'a kernel module that ...'; relation type 'depends_on' = 'X requires Y at runtime'). A description is required. Scoped to the workspace. Read them back via the supragnosis://workspace/{ws}/types resource."
    )]
    async fn define_type(&self, Parameters(req): Parameters<DefineTypeRequest>) -> String {
        if let Some(why) = self.all_is_no_place("define_type", &req.workspace) {
            return err_json(&why);
        }
        // Map the string target to the typed axis, rejecting anything else (self-correctable error).
        let mut defs = Vec::with_capacity(req.defs.len());
        for d in req.defs {
            let target = match d.target.trim().to_lowercase().as_str() {
                "entity" => TypeTarget::Entity,
                "relation" => TypeTarget::Relation,
                other => {
                    return err_json(&format!(
                        "unknown target '{other}' for type '{}'. use \"entity\" (a kind of thing) \
                         or \"relation\" (a kind of connection)",
                        d.name
                    ))
                }
            };
            defs.push(EngineTypeDefInput { target, name: d.name, description: d.description });
        }
        let written = req.workspace.clone();
        let input = DefineTypeInput {
            workspace: Some(req.workspace),
            source_ref: req.source_ref,
            on_behalf_of: req.on_behalf_of,
            defs,
        };
        let engine = self.engine.clone();
        let app = writing_app();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(app.as_deref(), || engine.define_type(input))
        })
        .await
        {
            Ok(Ok(observation_id)) => {
                serde_json::json!({ "observation_id": observation_id, "workspace": written })
                    .to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Open a proposal to change the canon - the one path by which anything enters shared knowledge. Use it to reshape what is already there rather than to add: entity_merge and entity_split for identity, claim_promotion and claim_demotion for trust, tbox_change for the type vocabulary, recall for retracting a contaminated derivation tree - those two have no commit effect yet, so neither can be merged until it has one. To state something new, use `observe` instead. A proposal is itself an observation and changes nothing until `review` accepts it; `get_proposal` shows where it stands, and `kind` describes what each one does and which arguments it needs. Returns the proposal id, the workspace, its state and found - true when an identical proposal was already there, since a proposal is its content - with a note saying what is left."
    )]
    async fn propose(&self, Parameters(req): Parameters<ProposeRequest>) -> String {
        if let Some(why) = self.all_is_no_place("propose", &req.workspace) {
            return err_json(&why);
        }
        // Map each affected type's string axis to the typed vocabulary (mirror define_type).
        let mut affected_types = Vec::with_capacity(req.affected_types.len());
        for a in req.affected_types {
            let target = match a.target.trim().to_lowercase().as_str() {
                "entity" => TypeTarget::Entity,
                "relation" => TypeTarget::Relation,
                other => {
                    return err_json(&format!(
                        "unknown target '{other}' for affected type '{}'. use \"entity\" \
                         (a node type) or \"relation\" (an edge type)",
                        a.name
                    ))
                }
            };
            affected_types.push(EngineAffectedType { target, name: a.name });
        }
        let written = req.workspace.clone();
        let note = no_effect_note(&req.kind);
        let input = EngineProposeInput {
            workspace: Some(req.workspace),
            kind: req.kind,
            targets: req.targets,
            into: req.into,
            tier: req.tier,
            rationale: req.rationale,
            affected_types,
            source_ref: req.source_ref,
            on_behalf_of: req.on_behalf_of,
        };
        let engine = self.engine.clone();
        let app = writing_app();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(app.as_deref(), || engine.propose_reporting(input))
        })
        .await
        {
            Ok(Ok(opened)) => {
                let mut answer = serde_json::json!({
                    "proposal_id": opened.id,
                    "workspace": written,
                    "state": opened.state,
                    "found": opened.found,
                });
                let note = [
                    note.map(|n| if opened.found { no_effect_found(&n) } else { n }),
                    opened.found.then(|| found_note(&opened.state)),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                if !note.is_empty() {
                    answer["note"] = note.join(". ").into();
                }
                answer.to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Cast a verdict on a proposal. decision is merge (accept), reject, comment, or withdraw. In a single-user workspace the verdict is self-attested. The state is a deterministic fold of the events; a merge is the absorbing outcome. A claim_promotion merged through this agent surface grants at most host_signed - promotion to human_confirmed is the person's own act in the Supragnosis app: never cast it yourself, by this tool or any other way. A merge on a recall is refused here for the same reason, and a merge on a tbox_change or a recall is refused on every surface while the kind has no commit effect; comment, reject and withdraw are accepted. Returns the recorded observation id and the workspace it was filed in, the proposal's own."
    )]
    async fn review(&self, Parameters(req): Parameters<ReviewRequest>) -> String {
        let engine = self.engine.clone();
        let (mut ws, prop, dec, note, obo) =
            (req.workspace, req.proposal, req.decision, req.note, req.on_behalf_of);
        // A verdict is filed in its proposal's workspace, the only place it counts (proposal-workflow.md
        // I2). This node may read every workspace, so an omitted one or `*` means the proposal's own
        // (design-system.md Section 29.15).
        if ws.as_deref().is_none_or(is_all) {
            match self.engine.proposal_workspace(&prop) {
                Ok(Some(home)) => ws = Some(home),
                Ok(None) => {
                    return err_json(&format!(
                        "no proposal {prop} in any workspace - ids come from propose and list_proposals"
                    ))
                }
                Err(e) => return store_failure_json(&e),
            }
        }
        let written = ws.clone().unwrap_or_else(|| self.engine.default_workspace().to_string());
        // This is the agent-facing surface (resolution.md Section 6): the engine stamps the Agent
        // marker, capping what a merged promotion can grant. Never client-suppliable.
        let app = writing_app();
        match tokio::task::spawn_blocking(move || {
            engine.as_app(app.as_deref(), || {
                engine.review_proposal(
                    ws,
                    prop,
                    dec,
                    note,
                    obo,
                    supragnosis_engine::VerdictSurface::Agent,
                )
            })
        })
        .await
        {
            Ok(Ok(id)) => {
                serde_json::json!({ "observation_id": id, "workspace": written }).to_string()
            }
            Ok(Err(e)) => err_json(&e.to_string()),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "List proposals in a workspace (newest first) with their folded state (open/blocked/merged/rejected/withdrawn), kind, targets, verdict count and the workspace each lives in; a merged claim_promotion also carries granted_tier, the tier it granted. A verdict counts only in its proposal's workspace. A tbox_change or a recall carries no_effect_yet: its kind has no commit effect yet, so it cannot be merged."
    )]
    async fn list_proposals(&self, Parameters(req): Parameters<ListProposalsRequest>) -> String {
        let ws = req.workspace.filter(|w| !is_all(w));
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.list_proposals(ws.as_deref())).await {
            Ok(Ok(list)) => to_json(&list),
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    #[tool(
        description = "Get one proposal's folded state (kind/targets/into/rationale/state/verdicts/workspace) by id, with its belief diff and checks, read in the workspace it lives in; a merged claim_promotion also carries granted_tier, the tier it granted. A tbox_change or a recall carries no_effect_yet: its kind has no commit effect yet, so it cannot be merged."
    )]
    async fn get_proposal(&self, Parameters(req): Parameters<GetProposalRequest>) -> String {
        let ws = req.workspace.filter(|w| !is_all(w));
        let id = req.id.clone();
        let engine = self.engine.clone();
        match tokio::task::spawn_blocking(move || engine.get_proposal(ws.as_deref(), &id)).await {
            Ok(Ok(Some(p))) => to_json(&p),
            // Open-world (Principle 5): absence is a well-formed answer, as for `get_entity` - an
            // error envelope would read as a failed call rather than as "not here".
            Ok(Ok(None)) => serde_json::json!({
                "found": false,
                "id": req.id,
                "note": "unknown - no proposal with that id in this scope, which is not a negation \
                         (open-world). A proposal lives in one workspace: pass workspace '*' to look \
                         in all of them, or use list_proposals",
            })
            .to_string(),
            Ok(Err(e)) => store_failure_json(&e),
            Err(e) => err_json(&format!("task join error: {e}")),
        }
    }

    // --- Federation sync tools (M4 Phase 4, docs/federation.md Section 9) ------------------------
    // Administrative surface over the sync client. Wire auth/trust semantics live in the sync layer
    // (F6/F10/F13); these tools only drive rounds and report. Without sync configuration they answer
    // with setup guidance (P5: an unconfigured capability is explained, never a silent failure).

    #[tool(
        description = "Federation sync status of this node: node id, public key, share whitelist, configured sync servers, the workspace's version vector (what this node holds per origin), `config_notes` (federation configuration this build worked around rather than obeyed - read these, they are the reason something is not syncing), and `negotiated` - what each host last said this node may reach, as a three-way difference against the share whitelist (`both` / `local_only` = listed here but not admitted there, a setup error / `peer_only` = admitted there but not shared here). A host with state `unknown` has not answered yet; that is not an empty grant set. Read-only."
    )]
    async fn sync_status(&self, Parameters(req): Parameters<SyncStatusRequest>) -> String {
        let Some(ctx) = &self.sync else {
            // A node that syncs with no hub may still be one: an invitation made its listener
            // while it ran, and the listener's state is said rather than "not configured".
            let listener = self
                .listener
                .as_ref()
                .and_then(|l| l.read().ok().map(|v| v.clone()))
                .filter(|v| v.get("listening").is_some());
            return match listener {
                Some(listener) => serde_json::json!({
                    "sync": "this node syncs with no hub; it is a hub, and its listener is as below",
                    "listener": listener,
                })
                .to_string(),
                None => sync_unconfigured(),
            };
        };
        let ws = req.workspace.unwrap_or_else(|| self.engine.default_workspace().to_string());
        let store = self.engine.store();
        let vv = match tokio::task::spawn_blocking({
            let ws = ws.clone();
            move || supragnosis_sync::version_vector(store.as_ref(), &ws)
        })
        .await
        {
            Ok(Ok(vv)) => vv,
            Ok(Err(e)) => {
                return err_json(&format!("store failure while reading the version vector: {e}"))
            }
            Err(e) => return err_json(&format!("task join error: {e}")),
        };
        // What each host admits for this node, against what this node shares - reported as the
        // three-way difference rather than the intersection, because the disagreement is the part
        // worth seeing: intersecting hides the misconfiguration that produced it (N6).
        let negotiated: Vec<serde_json::Value> = {
            let map = ctx.surfaces.read().ok();
            ctx.servers
                .iter()
                .map(|link| {
                    let entry = map.as_ref().and_then(|m| m.get(&link.url));
                    match entry.and_then(|e| e.admits.as_ref()) {
                        Some(admits) => {
                            let d = supragnosis_sync::surface_diff(&ctx.share_workspaces, admits);
                            serde_json::json!({
                                "server": link.url,
                                "state": "negotiated",
                                "negotiated_at": entry.and_then(|e| e.negotiated_at),
                                "both": d.both,
                                "local_only": d.local_only,
                                "peer_only": d.peer_only,
                            })
                        }
                        None => serde_json::json!({
                            "server": link.url,
                            "state": "unknown",
                            "note": "this host has not answered since the daemon started, or its \
                                     last health check failed. Unknown is not an empty grant set - \
                                     do not read it as a revoked grant",
                        }),
                    }
                })
                .collect()
        };
        let mut status = serde_json::json!({
            "config_notes": ctx.config_notes,
            "negotiated": negotiated,
            "node_id": ctx.node.node_id(),
            "public_key": ctx.node.public_key_hex(),
            "workspace": ws,
            "share_workspaces": ctx.share_workspaces,
            "servers": ctx.servers.iter().map(|l| &l.url).collect::<Vec<_>>(),
            "version_vector": vv,
        });
        // Hub role: the runtime known-peer registry (who checked in, when, how much - resets with
        // the process; the allowlist stays the admission authority).
        if let Some(reg) = &ctx.peer_registry {
            status["known_peers"] = serde_json::to_value(reg.snapshot()).unwrap_or_default();
        }
        // The daemon's own rounds (docs/pairing.md Section 4.5): when the last ran, what it moved,
        // and why it failed - null until one has run.
        status["last_round"] = ctx.last_round.read().map(|l| l.clone()).ok().flatten().into();
        status["listener"] = ctx.listener.read().map(|l| l.clone()).unwrap_or_default();
        to_json(&status)
    }

    #[tool(
        description = "Pull the workspace's missing knowledge from the configured sync servers and apply it (verify signatures -> dedup/absorb -> re-materialize the projection). Returns per-server accepted/rejected counts."
    )]
    async fn sync_pull(&self, Parameters(req): Parameters<SyncPullRequest>) -> String {
        let Some(ctx) = &self.sync else {
            return sync_unconfigured();
        };
        let ws = req.workspace.unwrap_or_else(|| self.engine.default_workspace().to_string());
        // A node a join set up pulls what it shares and what the join recorded, whatever a hub
        // would admit (docs/pairing.md Section 4.5, PA12).
        if let Some(receive) = &ctx.receive {
            if !ctx.share_workspaces.contains(&ws) && !receive.contains(&ws) {
                return err_json(&format!(
                    "this node pulls only the workspaces it shares and the ones its join recorded \
                     ({}); {ws} is neither",
                    ctx.share_workspaces
                        .iter()
                        .chain(receive)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        // One round at a time, whoever starts it (docs/pairing.md Section 4.5).
        let _round = ctx.round.lock().await;
        to_json(&pull_workspace(&self.engine, ctx, &ws).await)
    }

    #[tool(
        description = "Push the workspace's local knowledge to the configured sync servers (backfill-stamp unstamped attestations, then send what each server lacks per its advertised version vector). Only share-whitelisted workspaces leave this node."
    )]
    async fn sync_push(&self, Parameters(req): Parameters<SyncPushRequest>) -> String {
        let Some(ctx) = &self.sync else {
            return sync_unconfigured();
        };
        let ws = req.workspace.unwrap_or_else(|| self.engine.default_workspace().to_string());
        let _round = ctx.round.lock().await;
        to_json(&push_workspace(&self.engine, ctx, &ws).await)
    }
}

#[cfg(test)]
mod all_is_a_view {
    //! design-system.md Section 29.15 on the agent surface: `*` is a view, a verdict is filed where
    //! its proposal lives.
    use super::*;
    use supragnosis_core::Entity;
    use supragnosis_store::InMemoryStore;

    fn server() -> (Arc<Engine>, SupragnosisServer) {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        for (ws, names) in [
            ("infra", &["Payment service", "Payments"][..]),
            ("secret", &["Payment service"][..]),
        ] {
            engine
                .observe(ObserveInput {
                    content: format!("in {ws}"),
                    workspace: Some(ws.into()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: names
                        .iter()
                        .map(|n| EngineEntityInput {
                            name: (*n).into(),
                            kind: None,
                            description: None,
                        })
                        .collect(),
                    relations: vec![],
                })
                .expect("observe");
        }
        (engine.clone(), SupragnosisServer::new(engine))
    }

    fn req<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Parameters<T> {
        Parameters(serde_json::from_value(v).expect("request"))
    }

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("json")
    }

    #[tokio::test]
    async fn star_is_a_view_on_the_local_surface() {
        let (engine, s) = server();
        let out =
            json(&s.observe(req(serde_json::json!({ "content": "x", "workspace": "*" }))).await);
        assert!(
            out["error"].as_str().is_some_and(|e| e.contains("writes to one workspace")),
            "{out}"
        );
        assert!(
            !engine.workspaces().expect("list").iter().any(|w| w == "*"),
            "a workspace named * exists"
        );
        let hits = json(
            &s.search_knowledge(req(serde_json::json!({ "query": "Payments", "workspace": "*" })))
                .await,
        );
        assert!(
            hits["hits"].as_array().is_some_and(|h| !h.is_empty()),
            "`*` searched a workspace named *: {hits}"
        );
        // A verdict with no workspace is filed where its proposal lives.
        let here = Entity::make_id("infra", "Payment service");
        let p = json(
            &s.propose(req(serde_json::json!({
                "workspace": "infra", "kind": "entity_merge",
                "targets": [Entity::make_id("infra", "Payments"), here], "into": here,
            })))
            .await,
        );
        let pid = p["proposal_id"].as_str().expect("proposal id").to_string();
        let v = json(
            &s.review(req(serde_json::json!({ "proposal": pid, "decision": "reject" })))
                .await,
        );
        assert!(v["observation_id"].is_string(), "{v}");
        let state = engine.get_proposal(Some("infra"), &pid).expect("get").expect("proposal").state;
        assert_eq!(state, "rejected", "the verdict was not filed with its proposal");
        let v = json(
            &s.review(req(serde_json::json!({ "proposal": "nope", "decision": "comment" })))
                .await,
        );
        assert!(
            v["error"]
                .as_str()
                .is_some_and(|e| e.contains("no proposal nope in any workspace")),
            "{v}"
        );
    }
}

#[cfg(test)]
mod agent_answers {
    //! design-system.md Section 29.20: the agent surface says where it wrote, names its hits, says a
    //! name is one, and names the existing workspaces when a write makes a new one.
    use super::*;
    use supragnosis_store::InMemoryStore;

    fn req<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Parameters<T> {
        Parameters(serde_json::from_value(v).expect("request"))
    }

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).expect("json")
    }

    #[tokio::test]
    async fn a_write_says_where_and_a_new_workspace_is_said() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let s = SupragnosisServer::new(engine);
        let first = json(&s.observe(req(serde_json::json!({ "content": "a", "workspace": "acme-api", "entities": [{ "name": "A" }] }))).await);
        assert_eq!(first["workspace"], "acme-api");
        assert!(
            first["note"]
                .as_str()
                .is_some_and(|n| n.contains("this node held no knowledge before")),
            "{first}"
        );
        for ws in ["home-lab", "zeta", "acme-web"] {
            s.observe(req(
                serde_json::json!({ "content": ws, "workspace": ws, "entities": [{ "name": ws }] }),
            ))
            .await;
        }
        let again = json(
            &s.observe(req(serde_json::json!({ "content": "b", "workspace": "acme-api" })))
                .await,
        );
        assert!(
            again.get("note").is_none(),
            "a workspace that holds knowledge is said to be new"
        );
        let typo = json(
            &s.observe(req(serde_json::json!({ "content": "c", "workspace": "acme_api" })))
                .await,
        );
        let note = typo["note"].as_str().expect("a note for a new workspace");
        assert!(
            note.starts_with("workspace acme_api is new; the workspaces that hold knowledge are acme-api, acme-web"),
            "the nearest name is not first: {note}"
        );
        let p = json(&s.propose(req(serde_json::json!({
            "workspace": "acme-api", "kind": "entity_merge", "targets": ["x".repeat(64), "y".repeat(64)], "into": "y".repeat(64),
        }))).await);
        assert!(p["workspace"] == "acme-api" || p["error"].is_string(), "{p}");
    }

    #[tokio::test]
    async fn hits_name_their_entity_and_a_name_is_said_to_be_one() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        for ws in ["billing", "home-lab"] {
            engine
                .observe(ObserveInput {
                    content: format!("PostgreSQL in {ws}"),
                    workspace: Some(ws.into()),
                    source_ref: None,
                    confidence: None,
                    on_behalf_of: None,
                    derived_from: vec![],
                    entities: vec![EngineEntityInput {
                        name: "PostgreSQL".into(),
                        kind: Some("Database".into()),
                        description: None,
                    }],
                    relations: vec![],
                })
                .expect("observe");
        }
        let s = SupragnosisServer::new(engine);
        let found =
            json(&s.search_knowledge(req(serde_json::json!({ "query": "PostgreSQL" }))).await);
        let named: Vec<(String, String)> = found["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .filter(|h| h["kind"] == "entity")
            .map(|h| {
                (
                    h["workspace"].as_str().unwrap_or("").to_string(),
                    h["name"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        assert!(named.contains(&("billing".into(), "PostgreSQL".into())), "{found}");
        assert!(named.contains(&("home-lab".into(), "PostgreSQL".into())), "{found}");
        let by_name = json(&s.get_entity(req(serde_json::json!({ "id": "PostgreSQL" }))).await);
        assert!(
            by_name["note"].as_str().is_some_and(|n| n.contains("This looks like a name")),
            "{by_name}"
        );
        let by_id = json(&s.get_entity(req(serde_json::json!({ "id": "ab".repeat(32) }))).await);
        assert!(
            !by_id["note"].as_str().unwrap_or("").contains("looks like a name"),
            "an id read as a name"
        );
    }

    /// design-system.md Section 29.23: an identical proposal is the one already there, and the answer
    /// says what it found.
    #[tokio::test]
    async fn a_repeat_says_what_it_found() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let s = SupragnosisServer::new(engine);
        s.observe(req(serde_json::json!({ "content": "a and b", "workspace": "w", "entities": [{ "name": "A" }, { "name": "B" }] }))).await;
        let merge = serde_json::json!({ "workspace": "w", "kind": "entity_merge", "targets": ["A", "B"], "into": "A", "rationale": "one thing" });
        let first = json(&s.propose(req(merge.clone())).await);
        assert_eq!(
            (first["state"].as_str(), first["found"].as_bool()),
            (Some("open"), Some(false)),
            "{first}"
        );
        assert!(first.get("note").is_none());
        let again = json(&s.propose(req(merge.clone())).await);
        assert_eq!(again["proposal_id"], first["proposal_id"]);
        assert_eq!(again["found"], true);
        assert!(
            again["note"]
                .as_str()
                .is_some_and(|n| n.starts_with("an identical proposal is already open")),
            "{again}"
        );
        let id = first["proposal_id"].as_str().expect("id").to_string();
        s.review(req(serde_json::json!({ "proposal": id, "decision": "reject" }))).await;
        let rejected = json(&s.propose(req(merge)).await);
        assert_eq!(rejected["state"], "rejected");
        assert!(
            rejected["note"]
                .as_str()
                .is_some_and(|n| n.contains("a different rationale opens a new one")),
            "{rejected}"
        );
        let change = serde_json::json!({ "workspace": "w", "kind": "tbox_change", "targets": ["A"], "rationale": "a type" });
        s.propose(req(change.clone())).await;
        let found = json(&s.propose(req(change)).await);
        assert!(
            found["note"].as_str().is_some_and(|n| n.starts_with("tbox_change has no commit effect yet, so it cannot be merged until it has one. an identical proposal is already open")),
            "{found}"
        );
    }

    /// docs/pairing.md Section 4.5 and PA12: a node a join set up pulls what it shares and what the
    /// join recorded, and no other workspace, whatever a hub would admit.
    #[tokio::test]
    async fn a_joined_node_pulls_only_what_its_join_recorded() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let ctx = Arc::new(SyncContext {
            node: Arc::new(supragnosis_sync::SyncNode::new(
                supragnosis_core::NodeIdentity::from_secret_bytes([3; 32]),
            )),
            share_workspaces: vec!["acme".into()],
            receive: Some(vec!["acme".into(), "docs".into()]),
            vouched: Default::default(),
            servers: Vec::new(),
            config_notes: Vec::new(),
            surfaces: Default::default(),
            origin_keys: Default::default(),
            peer_registry: None,
            round: Default::default(),
            last_round: Default::default(),
            listener: Default::default(),
        });
        let s = SupragnosisServer::new(engine.clone()).with_sync(ctx.clone());
        let refused = json(&s.sync_pull(req(serde_json::json!({ "workspace": "other" }))).await);
        assert!(
            refused["error"].as_str().is_some_and(|e| e.contains("its join recorded")),
            "{refused}"
        );
        let docs = json(&s.sync_pull(req(serde_json::json!({ "workspace": "docs" }))).await);
        assert!(!docs.to_string().contains("its join recorded"), "{docs}");
        let round = sync_round(&engine, &ctx).await;
        assert_eq!(round["workspaces"], serde_json::json!(["acme", "docs"]), "{round}");
    }

    /// docs/pairing.md Section 4.1: a node with no sync that an invitation made a hub says what its
    /// listener is doing, rather than that sync is not configured.
    #[tokio::test]
    async fn a_hub_with_no_sync_says_what_its_listener_does() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let state: Arc<std::sync::RwLock<serde_json::Value>> = Default::default();
        let s = SupragnosisServer::new(engine).with_listener(state.clone());
        let before = json(&s.sync_status(req(serde_json::json!({}))).await);
        assert!(
            before["error"].as_str().is_some_and(|e| e.contains("not configured")),
            "{before}"
        );
        *state.write().unwrap() = serde_json::json!({"listening": false, "address": "0.0.0.0:7420", "why": "nothing is admitted"});
        let after = json(&s.sync_status(req(serde_json::json!({}))).await);
        assert_eq!(after["listener"]["why"], "nothing is admitted", "{after}");
        assert!(after.get("error").is_none());
    }

    /// design-system.md Section 29.22: a kind with no commit effect yet says so when it is opened and
    /// listed, and a merge of it is refused with why.
    #[tokio::test]
    async fn what_has_no_effect_says_so() {
        let engine = Arc::new(Engine::new(Arc::new(InMemoryStore::new()), "h", "default"));
        let s = SupragnosisServer::new(engine);
        s.observe(req(serde_json::json!({ "content": "a and b", "workspace": "w", "entities": [{ "name": "A" }, { "name": "B" }] }))).await;
        let opened = json(&s.propose(req(serde_json::json!({ "workspace": "w", "kind": "tbox_change", "targets": ["A"], "rationale": "a type" }))).await);
        assert!(
            opened["note"]
                .as_str()
                .is_some_and(|n| n.starts_with("tbox_change has no commit effect yet")),
            "{opened}"
        );
        let merge = json(&s.propose(req(serde_json::json!({ "workspace": "w", "kind": "entity_merge", "targets": ["A", "B"], "into": "A" }))).await);
        assert!(merge.get("note").is_none(), "a kind with an effect carries no note: {merge}");
        let id = opened["proposal_id"].as_str().expect("id").to_string();
        let refused =
            json(&s.review(req(serde_json::json!({ "proposal": id, "decision": "merge" }))).await);
        assert!(
            refused["error"]
                .as_str()
                .is_some_and(|e| e.contains("cannot be merged until it has one")),
            "{refused}"
        );
        let listed = json(&s.list_proposals(req(serde_json::json!({ "workspace": "w" }))).await);
        let row = listed
            .as_array()
            .expect("rows")
            .iter()
            .find(|p| p["id"] == id.as_str())
            .expect("row");
        assert_eq!(row["no_effect_yet"], true);
        assert_eq!(row["state"], "open", "the refused merge decided nothing");
    }
}
