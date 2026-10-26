//! Point reads: observations, entities, the observation log and entity explanations.

use super::*;

impl Engine {
    /// Observation dereference (Principle 2/14): from an observation id returned by a search hit / derivation
    /// lineage, reach the original text, the full provenance, and the derived_from lineage - the terminus of "where did this answer come from".
    pub fn get_observation(&self, id: &str) -> Result<Option<Observation>, StoreError> {
        self.store.get_observation(id)
    }

    /// Entity + relation lookup. `Ok(None)` is absence (unknown, Principle 5), `Err` is a store failure -
    /// failures are not swallowed, so the caller (the MCP surface) can distinguish and relay the two.
    pub fn get_entity(&self, id: &str) -> Result<Option<EntityView>, StoreError> {
        let cx = &ReadCtx::default();
        let Some(row) = self.store.get_entity(id)? else {
            return Ok(None);
        };
        let ws = row.provenance.first().map(|p| p.workspace.clone());
        let fwd = self.merge_forwarding(ws.as_deref(), cx)?;
        // Principle 14: a merged-away id keeps forwarding to its canonical entity - a lookup by any
        // pre-merge id dereferences to the surviving row (the log keeps both; un-merge is a proposal).
        let canon_id = fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
        let mut entity = if canon_id == *id {
            row
        } else {
            self.store.get_entity(&canon_id)?.unwrap_or(row)
        };
        let relations = self.merged_relations(&canon_id, &fwd)?;
        // Union merged-away names into aliases so get_entity sees the same alias set as the graph
        // fold (the write path already materialized same-id spelling aliases on the row - IR1).
        if !fwd.is_empty() {
            let mut merged: Vec<String> = self
                .store
                .all_entities(ws.as_deref())?
                .into_iter()
                .filter(|e| fwd.get(&e.id).is_some_and(|c| *c == canon_id))
                .map(|e| e.canonical_name)
                .filter(|n| *n != entity.canonical_name && !entity.aliases.contains(n))
                .collect();
            merged.sort();
            merged.dedup();
            entity.aliases.extend(merged);
        }
        // Belief overlay (resolution.md Section 4.2), scoped to this entity's workspace so the agent
        // surface sees the same policy-current kind/contested state as the viewer.
        let gates = self.gate_grants(ws.as_deref(), cx)?;
        let belief = self.belief_fold(ws.as_deref(), &fwd, &gates, cx)?;
        let (winner, contested, competitors) = self.resolve_kind(belief.kinds.get(&canon_id));
        let mut kind_source = None;
        if let Some((k, obs)) = winner {
            entity.kind = k;
            kind_source = Some(obs);
        }
        let effective_tier = belief.tiers.get(&canon_id).copied().unwrap_or_else(|| {
            entity.provenance.iter().map(evaluated_tier).max().unwrap_or_default()
        });
        // An edge's attestations are already this entity's or its neighbor's provenance; listing them
        // again on every relation would grow the answer with edges times sayings for nothing new.
        let mut relations = relations;
        for r in &mut relations {
            r.attestations.clear();
        }
        Ok(Some(EntityView {
            entity,
            relations,
            effective_tier,
            contested,
            competitors,
            kind_source,
        }))
    }

    /// The relations of an entity as the graph draws them (Principle 15): its own and every
    /// merged-away member's, endpoints forwarded to their canonical ids, a merge's self-loops
    /// dropped, and one row per (from, kind, to), the lowest relation id, as [`Engine::graph`] keeps.
    fn merged_relations(
        &self,
        canon_id: &str,
        fwd: &HashMap<String, String>,
    ) -> Result<Vec<Relation>, StoreError> {
        let canon = |id: &str| fwd.get(id).cloned().unwrap_or_else(|| id.to_string());
        let mut rows = self.store.relations_of(canon_id)?;
        for (member, c) in fwd {
            if c == canon_id {
                rows.extend(self.store.relations_of(member)?);
            }
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for mut r in rows {
            (r.from, r.to) = (canon(&r.from), canon(&r.to));
            if r.from != r.to && seen.insert((r.from.clone(), r.kind.clone(), r.to.clone())) {
                out.push(r);
            }
        }
        Ok(out)
    }

    /// The observation log for a workspace, flattened for the log-browser surface (observability).
    /// The log is the source of truth (Principle 1); this is a read-only projection of it, ordered
    /// newest-first by the deterministic fold key (ordering HLC desc, then id asc - P16), each row
    /// carrying its provenance (Principle 2) and effective tier (resolution.md Section 3). A storage
    /// failure is `Err`, never an empty log (Principle 5). When `entity` is given, only observations
    /// that touch that entity (an assertion or a relation endpoint, forwarded through accepted
    /// merges) are returned - the evidence set behind one node. `limit` keeps the newest N (None = all).
    pub fn observation_log(
        &self,
        workspace: Option<&str>,
        entity: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<ObsSummary>, StoreError> {
        self.observation_rows(workspace, entity, limit, None, false)
    }

    /// [`Engine::observation_log`] for the local viewer: each attestation names the app that wrote
    /// it where this node recorded one (design-system.md Section 29.8). Only the local console's
    /// routes call it; MCP reads [`Engine::observation_log`].
    pub fn observation_log_local(
        &self,
        workspace: Option<&str>,
        entity: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<ObsSummary>, StoreError> {
        self.observation_rows(workspace, entity, limit, None, true)
    }

    /// One observation by its id, as [`Engine::observation_log`] answers it in `workspace` (every
    /// workspace when `None`), so a card opens without reading a scope's log (design-system.md
    /// Section 29.7). `None` for an id the scope does not hold (absence, Principle 5).
    pub fn observation(
        &self,
        id: &str,
        workspace: Option<&str>,
    ) -> Result<Option<ObsSummary>, StoreError> {
        Ok(self
            .observation_rows(workspace, None, None, Some(id), false)?
            .into_iter()
            .next())
    }

    /// [`Engine::observation`] for the local viewer, naming each attestation's app.
    pub fn observation_local(
        &self,
        id: &str,
        workspace: Option<&str>,
    ) -> Result<Option<ObsSummary>, StoreError> {
        Ok(self.observation_rows(workspace, None, None, Some(id), true)?.into_iter().next())
    }

    /// The workspace an observation was written to, or `None` for an id no workspace holds - what
    /// the read tier checks against a principal's grants before it answers by id.
    pub fn observation_workspace(&self, id: &str) -> Result<Option<String>, StoreError> {
        let cx = &ReadCtx::default();
        Ok(self
            .log(None, cx)?
            .iter()
            .find(|o| o.id == id)
            .map(|o| o.workspace().to_string()))
    }

    fn observation_rows(
        &self,
        workspace: Option<&str>,
        entity: Option<&str>,
        limit: Option<usize>,
        only: Option<&str>,
        apps: bool,
    ) -> Result<Vec<ObsSummary>, StoreError> {
        let cx = &ReadCtx::default();
        let fwd = self.merge_forwarding(workspace, cx)?;
        let (gates, moved) = self.gate_fold(workspace, cx, &[], None)?;
        // Each moving event's verdict, said by its authoring attestation: when, at which surface,
        // from which sync origin (design-system.md Section 29.7).
        let verdicts: HashSet<&str> = moved.values().map(|m| m.1.as_str()).collect();
        let mut verdict_said: HashMap<String, (Timestamp, Option<String>, Option<String>)> =
            HashMap::new();
        if !verdicts.is_empty() {
            for obs in self.log(workspace, cx)?.iter() {
                if !verdicts.contains(obs.id.as_str()) {
                    continue;
                }
                if let Some(a) = authoring_attestation(obs) {
                    verdict_said.insert(
                        obs.id.clone(),
                        (
                            a.observed_at,
                            a.source_ref.clone(),
                            a.sync.as_ref().map(|s| s.origin_node.clone()),
                        ),
                    );
                }
            }
        }
        let gated_by = |id: &str| {
            moved.get(id).map(|(proposal, verdict, promotion)| {
                let (at, surface, origin_node) =
                    verdict_said.get(verdict).cloned().unwrap_or((0, None, None));
                GateMove {
                    proposal: proposal.clone(),
                    promotion: *promotion,
                    verdict: verdict.clone(),
                    at,
                    surface,
                    origin_node,
                }
            })
        };
        let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);
        let want = entity.map(|e| canon(e.to_string()));
        // Proposal-event rows are unreadable as stored (machine text, fixed inside the content
        // address), so they are translated below. Both inputs are loaded ONLY if such a row exists:
        // the fold is over rows already in the ReadCtx, and the name lookup is the one store scan,
        // which a workspace with no proposals never pays. The log panel is opened on demand, not
        // polled like the graph, so that scan is affordable where it would not be on the poll path.
        let has_events = self
            .log(workspace, cx)?
            .iter()
            .any(|o| !o.assertions.proposal_events.is_empty());
        let proposals =
            if has_events { self.fold_proposals(workspace, cx)? } else { BTreeMap::new() };
        let names: BTreeMap<String, String> = if has_events {
            self.store
                .all_entities(workspace)?
                .into_iter()
                .map(|e| (e.id, e.canonical_name))
                .collect()
        } else {
            BTreeMap::new()
        };
        // A target that is not an entity - a gate act's observation, an un-merge's merge - is said as
        // the proposal rows say it (design-system.md Section 29.3).
        let mut labels: BTreeMap<String, String> = BTreeMap::new();
        for (pid, v) in &proposals {
            if v.targets.iter().chain(v.into.iter()).all(|t| names.contains_key(t)) {
                continue;
            }
            let (ls, into) = self.labels_for(pid, v, &proposals)?;
            for (t, l) in v.targets.iter().zip(ls).chain(v.into.iter().zip([into])) {
                if let (false, Some(l)) = (names.contains_key(t), l) {
                    labels.entry(t.clone()).or_insert(l);
                }
            }
        }
        // A target keeps its id when nothing can name it - a merged-away row still resolves here
        // because the entity table keeps it; only a target this node has never projected stays bare.
        let eref = |id: &str| EntityRef {
            name: names
                .get(id)
                .or_else(|| labels.get(id))
                .cloned()
                .unwrap_or_else(|| id.to_string()),
            id: id.to_string(),
        };
        let mut out: Vec<ObsSummary> = Vec::new();
        // Each row's attestation keys, in its attestations' order, when the read names apps.
        let mut app_keys: HashMap<String, Vec<String>> = HashMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            if only.is_some_and(|o| o != obs.id) {
                continue;
            }
            let ws = obs.workspace().to_string();
            // Canonical entity refs this observation asserts (assertions + relation endpoints),
            // keyed by canonical id (deterministic BTreeMap order), first spelling kept for display.
            let mut ent_ids: BTreeMap<String, String> = BTreeMap::new();
            for ea in &obs.assertions.entities {
                ent_ids
                    .entry(canon(Entity::make_id(&ws, &ea.name)))
                    .or_insert_with(|| ea.name.clone());
            }
            for ra in &obs.assertions.relations {
                for name in [&ra.from, &ra.to] {
                    ent_ids
                        .entry(canon(Entity::make_id(&ws, name)))
                        .or_insert_with(|| name.clone());
                }
            }
            if let Some(w) = &want {
                if !ent_ids.contains_key(w) {
                    continue;
                }
            }
            let attestations: Vec<AttestationSummary> = obs
                .provenance
                .iter()
                .map(|p| AttestationSummary {
                    app: None,
                    host: p.host.clone(),
                    on_behalf_of: p.on_behalf_of.clone(),
                    source_ref: p.source_ref.clone(),
                    observed_at: p.observed_at,
                    confidence: p.confidence,
                    trust_tier: p.trust_tier,
                    evaluated_tier: evaluated_tier(p),
                    origin_node: p.sync.as_ref().map(|s| s.origin_node.clone()),
                })
                .collect();
            let entities: Vec<EntityRef> =
                ent_ids.into_iter().map(|(id, name)| EntityRef { name, id }).collect();
            // The readable form of a proposal event. `opened` names its own proposal (the id IS this
            // observation); every other event points at one, and the view supplies kind/targets that
            // the event itself does not carry.
            let proposal = obs.assertions.proposal_events.first().map(|ev| {
                let pid = if ev.proposal.is_empty() { obs.id.clone() } else { ev.proposal.clone() };
                let payload: serde_json::Value =
                    serde_json::from_str(&ev.payload).unwrap_or(serde_json::Value::Null);
                let view = proposals.get(&pid);
                let decision = payload.get("decision").and_then(|d| d.as_str());
                let (noun, verb) =
                    proposal_words(view.map_or("", |v| v.kind.as_str()), ev.event, decision);
                ProposalEventSummary {
                    event: match ev.event {
                        ProposalEventKind::Opened => "opened",
                        ProposalEventKind::Verdict => "verdict",
                        ProposalEventKind::Withdrawn => "withdrawn",
                        ProposalEventKind::Comment => "comment",
                    }
                    .to_string(),
                    decision: decision.map(str::to_string),
                    kind: view.map(|v| v.kind.clone()).unwrap_or_default(),
                    state: view.map(|v| v.state.clone()).unwrap_or_default(),
                    targets: view
                        .map(|v| v.targets.iter().map(|t| eref(t)).collect())
                        .unwrap_or_default(),
                    into: view.and_then(|v| v.into.as_deref()).map(eref),
                    noun,
                    verb,
                    proposal: pid,
                }
            });
            let relations = obs
                .assertions
                .relations
                .iter()
                .map(|ra| RelationRef {
                    from: ra.from.clone(),
                    kind: normalize_relation_kind(&ra.kind),
                    to: ra.to.clone(),
                })
                .collect();
            if apps {
                app_keys.insert(
                    obs.id.clone(),
                    obs.provenance.iter().map(|p| attestation_key(&obs.id, p)).collect(),
                );
            }
            out.push(ObsSummary {
                hlc: ordering_hlc(obs),
                effective_tier: effective_tier(obs, &gates),
                id: obs.id.clone(),
                workspace: ws,
                content: obs.content.clone(),
                proposal,
                derived_from: obs.derived_from.clone(),
                attestations,
                author: authoring_index(obs).unwrap_or(0),
                gated_by: gated_by(&obs.id),
                entities,
                relations,
            });
        }
        // Newest first: descending ordering HLC, ascending id as the stable tiebreak (P16).
        out.sort_by(|a, b| b.hlc.cmp(&a.hlc).then_with(|| a.id.cmp(&b.id)));
        if let Some(n) = limit {
            out.truncate(n);
        }
        if apps {
            self.name_apps(&mut out, &app_keys)?;
        }
        Ok(out)
    }

    /// Names each attestation's app where this node recorded one (design-system.md Section 29.8).
    /// A row exists only for what this node's own sessions wrote, so a peer's attestation is never
    /// named. A backend failure is an error, not "no app" (Principle 5).
    fn name_apps(
        &self,
        rows: &mut [ObsSummary],
        app_keys: &HashMap<String, Vec<String>>,
    ) -> Result<(), StoreError> {
        let mut keys: Vec<String> = Vec::new();
        let mut at: Vec<(usize, usize, String)> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            for (j, key) in app_keys.get(&row.id).into_iter().flatten().enumerate() {
                keys.push(key.clone());
                at.push((i, j, key.clone()));
            }
        }
        if keys.is_empty() {
            return Ok(());
        }
        let found = self.store.attestation_apps(&keys)?;
        for (i, j, key) in at {
            if let (Some(name), Some(a)) = (found.get(&key), rows[i].attestations.get_mut(j)) {
                a.app = Some(supragnosis_core::client_said(name));
            }
        }
        Ok(())
    }

    /// "Why is this node projected this way" (observability): the per-field belief resolution
    /// (evidence + decision) and the supporting observation log for one entity. Built ON TOP of
    /// [`Engine::get_entity`] - the winner values ARE the projected entity's values, so the
    /// explanation cannot disagree with what the graph shows (never a second computation that could
    /// drift). The candidate rows and supporting log are folded from the same log the projection
    /// uses. `None` iff the id resolves to no entity (absence, Principle 5). A merged-away id
    /// forwards to its canonical entity (Principle 15), like [`Engine::get_entity`].
    pub fn explain_entity(&self, entity_id: &str) -> Result<Option<EntityExplain>, StoreError> {
        self.explain_entity_as(entity_id, false)
    }

    /// [`Engine::explain_entity`] for the local viewer: its supporting rows name each attestation's
    /// app (design-system.md Section 29.8).
    pub fn explain_entity_local(
        &self,
        entity_id: &str,
    ) -> Result<Option<EntityExplain>, StoreError> {
        self.explain_entity_as(entity_id, true)
    }

    fn explain_entity_as(
        &self,
        entity_id: &str,
        apps: bool,
    ) -> Result<Option<EntityExplain>, StoreError> {
        let cx = &ReadCtx::default();
        let Some(view) = self.get_entity(entity_id)? else {
            return Ok(None);
        };
        let canon_id = view.entity.id.clone();
        let ws = view.entity.provenance.first().map(|p| p.workspace.clone());
        let fwd = self.merge_forwarding(ws.as_deref(), cx)?;
        let gates = self.gate_grants(ws.as_deref(), cx)?;
        let canon = |id: String| fwd.get(&id).cloned().unwrap_or(id);

        // Rebuild this entity's name/kind belief candidates from the log, at the same effective tier
        // and fold order as materialize_entities/belief_fold - so the ranking matches the projection.
        let mut name_cands: Vec<BeliefCandidate> = Vec::new();
        let mut kind_cands: Vec<BeliefCandidate> = Vec::new();
        let mut bases: HashMap<String, TrustTier> = HashMap::new();
        for obs in self.log(ws.as_deref(), cx)?.iter() {
            let obs_ws = obs.workspace().to_string();
            let eff = effective_tier(obs, &gates);
            bases.insert(
                obs.id.clone(),
                obs.provenance.iter().map(evaluated_tier).max().unwrap_or_default(),
            );
            let hlc = ordering_hlc(obs);
            for ea in &obs.assertions.entities {
                if canon(Entity::make_id(&obs_ws, &ea.name)) != canon_id {
                    continue;
                }
                name_cands.push(BeliefCandidate {
                    value: ea.name.trim().to_string(),
                    tier: eff,
                    hlc: hlc.clone(),
                    observation: obs.id.clone(),
                });
                if let Some(k) = &ea.kind {
                    kind_cands.push(BeliefCandidate {
                        value: k.clone(),
                        tier: eff,
                        hlc: hlc.clone(),
                        observation: obs.id.clone(),
                    });
                }
            }
            for ra in &obs.assertions.relations {
                for name in [&ra.from, &ra.to] {
                    if canon(Entity::make_id(&obs_ws, name)) == canon_id {
                        name_cands.push(BeliefCandidate {
                            value: name.trim().to_string(),
                            tier: eff,
                            hlc: hlc.clone(),
                            observation: obs.id.clone(),
                        });
                    }
                }
            }
        }

        // canonical_name: the projected name is the winner; every other spelling is an alias (IR1).
        let name_contested = self.policy.choose(&name_cands).map(|c| c.contested).unwrap_or(false);
        let fields = vec![
            FieldExplain {
                field: "canonical_name",
                winner: view.entity.canonical_name.clone(),
                contested: name_contested,
                candidates: candidate_rows(
                    &name_cands,
                    &view.entity.canonical_name,
                    "alias",
                    &bases,
                ),
            },
            // kind: the projected kind is the winner; non-winning kinds are competitors (R7).
            // contested is get_entity's own flag (the identical resolve_kind result).
            FieldExplain {
                field: "kind",
                winner: view.entity.kind.clone(),
                contested: view.contested,
                candidates: candidate_rows(&kind_cands, &view.entity.kind, "competitor", &bases),
            },
        ];

        let supporting = self.observation_rows(ws.as_deref(), Some(&canon_id), None, None, apps)?;
        Ok(Some(EntityExplain {
            id: canon_id,
            name: view.entity.canonical_name,
            effective_tier: view.effective_tier,
            fields,
            supporting,
        }))
    }
}
