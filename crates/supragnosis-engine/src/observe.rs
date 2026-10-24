//! Ingest: the secret scan, event emission, app attribution, `observe`, `define_type` and the
//! type glossary.

use super::*;

/// The core ontology's meta-level types (principles.md P10). The core is fixed and the domain
/// vocabulary extends it through `define_type`, so these names are refused there: a domain type
/// called `Observation` would put a second meaning on the word the whole model is built on (P8).
/// Refused at the local door only. A peer's definition is not refused on arrival, since a node
/// that did would diverge from one that had already accepted it (P16).
const CORE_META_TYPES: [&str; 6] =
    ["Observation", "Entity", "Relation", "Provenance", "Host", "Workspace"];

/// Every free-text field of an observation that can carry a secret, with the name the operator sees.
///
/// One list, shared by the ingest refusal and the stored-log scan. Two walks would drift, and the
/// drift would be silent in the worst direction: a field the door checks but the scan does not means
/// a secret that was refused at ingest, arrived some other way, and is then reported as absent.
pub(crate) fn scannable_fields(obs: &Observation) -> Vec<(&'static str, &str)> {
    let mut fields: Vec<(&'static str, &str)> = vec![("content", obs.content.as_str())];
    for p in &obs.provenance {
        if let Some(r) = &p.source_ref {
            fields.push(("source_ref", r.as_str()));
        }
    }
    for e in &obs.assertions.entities {
        fields.push(("entity name", e.name.as_str()));
        if let Some(d) = &e.description {
            fields.push(("entity description", d.as_str()));
        }
    }
    for r in &obs.assertions.relations {
        if let Some(d) = &r.description {
            fields.push(("relation description", d.as_str()));
        }
    }
    for t in &obs.assertions.type_defs {
        fields.push(("type description", t.description.as_str()));
    }
    for ev in &obs.assertions.proposal_events {
        fields.push(("proposal payload", ev.payload.as_str()));
    }
    fields
}

/// Refuses a client-supplied source_ref inside the reserved verdict-marker namespace
/// (resolution.md Section 6, R8). The ceiling fold trusts a log-borne marker to be engine-stamped,
/// so every local ingest door except `review_proposal` (which stamps its own marker and accepts no
/// source_ref) must refuse the prefix. Sync apply is deliberately unguarded: a replicated verdict
/// legitimately carries its marker, and refusing it would break the ceiling's convergence.
pub(crate) fn reject_reserved_source_ref(source_ref: Option<&str>) -> Result<(), ObserveError> {
    if let Some(s) = source_ref {
        if s.starts_with(VERDICT_SURFACE_PREFIX) {
            return Err(ObserveError::Invalid(format!(
                "source_ref '{s}' is inside the reserved '{VERDICT_SURFACE_PREFIX}' namespace - \
                 verdict-surface markers are engine-stamped and cannot be supplied by a client. \
                 Name the actual source instead (a file path, URL, or tool)"
            )));
        }
    }
    Ok(())
}

impl Engine {
    /// Refuses to append an observation carrying credential-shaped text (Principle 17: the
    /// secret-redaction hook at ingest).
    ///
    /// **Every local door, and only the local doors.** It deliberately does NOT run on the sync apply
    /// path. `check_well_formed` can run there because structural validity is stable across versions,
    /// but a detector's patterns grow - so a node on a newer build would refuse an event an older peer
    /// accepted, and the two would hold different logs from the same event set. That is precisely the
    /// P16 divergence the apply path exists to avoid. This is a node-local ingest aid, which is what
    /// P17 asks for: "an aid to, not a replacement for, the sharing filter".
    ///
    /// The refusal names the field and the shape and never the value, so declining the write cannot
    /// itself publish the secret (excision.md E2). The message is written for an agent to act on
    /// without a human in the loop (P21).
    pub(crate) fn refuse_secrets(&self, obs: &Observation) -> Result<(), ObserveError> {
        if !self.scan_secrets {
            return Ok(());
        }
        for (field, text) in scannable_fields(obs) {
            if let Some(hit) = supragnosis_core::detect_secret(text) {
                return Err(ObserveError::Invalid(format!(
                    "refusing to store this: the {field} contains something shaped like a credential \
                     ({}, at byte {}). The log is append-only, so anything written here cannot be \
                     taken back - and if this workspace is shared, it replicates. Remove the secret \
                     and observe the knowledge without it: say that a credential exists and where it \
                     is configured, never what it is",
                    hit.pattern, hit.at
                )));
            }
        }
        Ok(())
    }

    /// Emits a UI event. Does nothing if there is no sink (observability is optional).
    /// Carries the session id in the envelope (footprint group key). The caller (MCP tool handler) invokes it per
    /// intent - a side channel unrelated to the storage/resolution logic.
    pub fn emit(&self, event: Event) {
        self.emit_from(None, event);
    }

    /// [`Engine::emit`], naming the app of the session the activity came from - a local MCP
    /// session's, never a remote principal's (design-system.md Section 29.8).
    pub fn emit_from(&self, app: Option<&str>, event: Event) {
        if let Some(sink) = &self.events {
            sink.emit(&EventEnvelope {
                session: self.session.clone(),
                // By the name a person reads (the core's table), as an attestation's app is said.
                app: app.map(supragnosis_core::client_said),
                event,
            });
        }
    }

    /// Runs `f` with what it writes attributed to `app`, the client name of the local MCP session
    /// that asked (design-system.md Section 29.8): each attestation `f` adds records it, in this
    /// node's own table. `None` attributes nothing. The attribution is the calling thread's, as the
    /// write is.
    pub fn as_app<T>(&self, app: Option<&str>, f: impl FnOnce() -> T) -> T {
        let was = WRITING_APP.with(|a| a.replace(app.and_then(supragnosis_core::client_recorded)));
        let out = f();
        WRITING_APP.with(|a| a.replace(was));
        out
    }

    /// Records the app this thread writes for, under the attestation's key. Recording never fails
    /// the write it belongs to (P24): a failure is logged and the write stands.
    pub(crate) fn note_app(&self, key: Option<&str>) {
        let (Some(key), Some(app)) = (key, WRITING_APP.with(|a| a.borrow().clone())) else {
            return;
        };
        if let Err(e) = self.store.record_attestation_app(key, &app) {
            tracing::warn!(error = %e, "the app that wrote an attestation was not recorded");
        }
    }

    pub(crate) fn provenance(
        &self,
        workspace: &str,
        source_ref: Option<String>,
        confidence: Option<f32>,
        on_behalf_of: Option<String>,
    ) -> Provenance {
        Provenance {
            host: self.host.clone(),
            on_behalf_of,
            workspace: workspace.to_string(),
            source_ref,
            observed_at: self.clock.now_millis(),
            // Preserve no-annotation as no-annotation (Principle 2, 4th) - substituting a default (1.0) is a
            // capture loss that erases the distinction between "no assertion" and "full-confidence assertion". Interpretation is the resolution policy's job (M3).
            confidence,
            // Trust tier promotion only happens in an explicit flow (human confirmation / cross-validation) - observe uses the default.
            trust_tier: TrustTier::default(),
            sync: None,
        }
    }

    /// Ingests a piece of knowledge: stores an immutable observation + links the provided entities/relations into the ontology.
    pub fn observe(&self, input: ObserveInput) -> Result<ObserveOutput, ObserveError> {
        // A bound every event fits inside (sync-correctness.md Section 11): without it one
        // observation could exceed any sync body limit on its own, and a peer's stream would be held
        // forever behind an event that can never be delivered. Applied at local ingest only - an
        // event already signed elsewhere is not refused for its size.
        if input.content.len() > MAX_CONTENT_BYTES {
            return Err(ObserveError::Invalid(format!(
                "content is {} bytes; an observation holds at most {MAX_CONTENT_BYTES} (1 MiB). \
                 Split it into observations that each say one thing, or keep the large text \
                 elsewhere and observe what it says with a source_ref pointing at it",
                input.content.len()
            )));
        }
        // The verdict-marker namespace is engine-controlled provenance, like trust_tier and
        // observed_at - a client-supplied "surface:*" is a forged provenance claim, not content.
        reject_reserved_source_ref(input.source_ref.as_deref())?;
        // Enforce the confidence range (Principle 2: schema-level enforcement). A value once written to the
        // append-only log is permanent, so we block it before ingest. NaN is caught too, since contains is false for it.
        if let Some(c) = input.confidence {
            if !(0.0..=1.0).contains(&c) {
                return Err(ObserveError::Invalid(format!(
                    "confidence must be in the range 0.0~1.0 (received: {c}). If confidence is low, \
                     give a low value; if it cannot be evaluated, omit it"
                )));
            }
        }
        // Well-formedness validation (Principle 1: ingest validation goes only as far as well-formedness). An empty
        // directive is not a "differently spelled assertion" but a non-assertion with no referent - block it before it
        // reaches the permanent log. The notation itself is not censored: rejection is not transformation, and normalization is the projection's job.
        for e in &input.entities {
            if e.name.trim().is_empty() {
                return Err(ObserveError::Invalid(
                    "entity name is empty. an entity assertion with no name does not hold - \
                     provide a name to refer to, or drop the item"
                        .into(),
                ));
            }
            if e.kind.as_deref().is_some_and(|k| k.trim().is_empty()) {
                return Err(ObserveError::Invalid(format!(
                    "the type of entity '{}' is an empty string. an empty-type assertion is a \
                     non-holding assertion, different from leaving the type unspecified - if you don't know the type, omit type",
                    e.name
                )));
            }
        }
        for r in &input.relations {
            if r.from.trim().is_empty() || r.to.trim().is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "a relation endpoint is empty (from: {:?}, to: {:?}). a relation assertion that \
                     points to an unnamed entity does not hold - provide both endpoint entity names",
                    r.from, r.to
                )));
            }
            if normalize_relation_kind(&r.kind).is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "the relation type is empty (received: {:?} - normalizes to an empty string). \
                     provide a type whose meaning reads clearly, like depends_on / part_of",
                    r.kind
                )));
            }
        }
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());
        let prov =
            self.provenance(&workspace, input.source_ref, input.confidence, input.on_behalf_of);

        // Structured assertions are enclosed in the observation log **verbatim** (Principle 1: the log is the source
        // of truth and the graph is a projection - if an assertion is not in the log, the graph cannot be recovered by
        // re-projection). Normalization (kind canonicalization, etc.) is the job of the projection step below.
        let assertions = Assertions {
            entities: input
                .entities
                .iter()
                .map(|e| EntityAssertion {
                    name: e.name.clone(),
                    kind: e.kind.clone(),
                    description: e.description.clone(),
                })
                .collect(),
            relations: input
                .relations
                .iter()
                .map(|r| RelationAssertion {
                    from: r.from.clone(),
                    kind: r.kind.clone(),
                    to: r.to.clone(),
                    description: r.description.clone(),
                    valid_from: r.valid_from,
                    valid_to: r.valid_to,
                })
                .collect(),
            // observe does not define types or open proposals (those are the define_type/propose intents).
            type_defs: Vec::new(),
            proposal_events: Vec::new(),
        };
        let mut obs = Observation::with_assertions(input.content, prov.clone(), assertions);
        obs.derived_from = input.derived_from;
        // Embedding attachment is best-effort: a failure does not block storing the observation (Principle 19: degrade).
        // But degrade is not silent: an embedding failure at ingest time excludes this observation from semantic
        // search with no retry (until the same content is re-observed), so it leaves a trace.
        if let Some(embedder) = &self.embedder {
            match embedder.embed_one(&obs.content) {
                Ok(vec) => obs.embedding = Some(vec),
                Err(e) => tracing::warn!(
                    observation_id = %obs.id,
                    error = %e,
                    "observation embedding failed - recalled by keyword search only (degrade)"
                ),
            }
        }
        let observation_id = obs.id.clone();
        // Serialize the write section (prevents the read-merge-write race of concurrent observations' projections, see the `write_guard` field comment in lib.rs).
        // Embedding (above, probabilistic/CPU) is left outside the lock; we lock from here. The read path is not locked.
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refuse_secrets(&obs)?;
        let app_key = obs.provenance.first().map(|p| attestation_key(&obs.id, p));

        // Resolution write path (resolution-identity.md Section 4): (re)project exactly the entities
        // the observation touches - the same fold reproject runs, so the incremental row equals a
        // fresh replay's (IR3). Relations project the same way (their id is deterministic and
        // description/valid-interval are last-write, matching reproject).
        let entity_ids: Vec<String> =
            input.entities.iter().map(|e| Entity::make_id(&workspace, &e.name)).collect();
        let mut touched: HashSet<String> = entity_ids.iter().cloned().collect();
        for r in &input.relations {
            touched.insert(Entity::make_id(&workspace, &r.from));
            touched.insert(Entity::make_id(&workspace, &r.to));
        }
        // The edge ids this call asserted. Derived from the input rather than from the projection,
        // because the two answer different questions: this is "what did this observe name", while the
        // projection decides what each of those edges now looks like given the whole log.
        let relations: Vec<String> = input
            .relations
            .iter()
            .map(|r| {
                Relation::make_id(
                    &Entity::make_id(&workspace, &r.from),
                    &normalize_relation_kind(&r.kind),
                    &Entity::make_id(&workspace, &r.to),
                )
            })
            .collect();
        let touched_edges: HashSet<String> = relations.iter().cloned().collect();

        // The row and its projection commit in one transaction (crash-recovery.md Section 3a), so
        // the projection is folded from the log as it will read with the row in it: the stored row
        // of this id, absorbed with the new attestation, or the new row when there is none. The read
        // context carries it to every fold, the gate grants included - a merged promotion of this
        // very row, synced in before it, applies once the row is here, as a replay applies it.
        let row = match self.store.get_observation(&obs.id)? {
            Some(mut stored) => {
                stored.absorb(obs.clone());
                stored
            }
            None => obs.clone(),
        };
        let cx = &ReadCtx::over(row);
        let entities =
            self.materialize_entities(&workspace, Some(&touched), cx, &HashMap::new())?;
        let edges = self.relation_rows(&workspace, Some(&touched_edges), cx.overlay.as_ref())?;
        // Nothing is owed: the row lands with its projection or not at all (K1), and no entry the
        // ledger already holds for this id - a sync apply of the same content - is cleared (K2).
        self.store.commit_projected(obs, entities, edges)?;
        self.note_app(app_key.as_deref());
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        Ok(ObserveOutput { observation_id, entities: entity_ids, relations })
    }

    /// Records T-Box type definitions as an observation (Principle 8/11: an explicit define_type act,
    /// scoped to the workspace). It rides the observation log like any other assertion (Principle 1/23:
    /// no parallel provenance/storage), so the glossary is a deterministic projection
    /// ([`Engine::types`]) and a future proposal gate (Principle 23) can wrap this without rework.
    /// Principles 8 and 10 are enforced here: a definition with no name, an empty description, or a
    /// core meta-type's name is rejected before it reaches the permanent log.
    pub fn define_type(&self, input: DefineTypeInput) -> Result<String, ObserveError> {
        reject_reserved_source_ref(input.source_ref.as_deref())?;
        if input.defs.is_empty() {
            return Err(ObserveError::Invalid(
                "no type definitions provided. give at least one {target, name, description}"
                    .into(),
            ));
        }
        for d in &input.defs {
            if d.name.trim().is_empty() {
                return Err(ObserveError::Invalid(
                    "a type definition has an empty name - name the type you are defining".into(),
                ));
            }
            // Principle 10: the core ontology is fixed, so its meta-level names are not domain types.
            if let Some(core) =
                CORE_META_TYPES.iter().find(|c| c.eq_ignore_ascii_case(d.name.trim()))
            {
                return Err(ObserveError::Invalid(format!(
                    "type '{}' is the core ontology's {core}, and the core is fixed (Principle 10) - \
                     name the domain concept you mean instead, such as 'MeetingNote' or 'Service'",
                    d.name.trim()
                )));
            }
            // Principle 8 (Clarity): a type cannot be created without a natural-language definition.
            if d.description.trim().is_empty() {
                return Err(ObserveError::Invalid(format!(
                    "type '{}' has an empty description. Principle 8 (clarity): a type needs a \
                     natural-language definition of what it means - describe it, or drop the item",
                    d.name
                )));
            }
        }
        let workspace = input.workspace.unwrap_or_else(|| self.default_workspace.clone());
        let prov = self.provenance(&workspace, input.source_ref, None, input.on_behalf_of);

        // Synthesize readable content so the definition is also keyword/semantic searchable (Principle 22).
        let mut content = String::from("Type definitions:");
        for d in &input.defs {
            let axis = match d.target {
                TypeTarget::Entity => "entity",
                TypeTarget::Relation => "relation",
            };
            content.push_str(&format!(
                "\n- {axis} type `{}`: {}",
                d.name.trim(),
                d.description.trim()
            ));
        }
        let assertions = Assertions {
            type_defs: input
                .defs
                .into_iter()
                .map(|d| TypeDefAssertion {
                    target: d.target,
                    name: d.name.trim().to_string(),
                    description: d.description.trim().to_string(),
                })
                .collect(),
            ..Default::default()
        };
        let mut obs = Observation::with_assertions(content, prov.clone(), assertions);
        if let Some(embedder) = &self.embedder {
            if let Ok(vec) = embedder.embed_one(&obs.content) {
                obs.embedding = Some(vec);
            }
        }
        let observation_id = obs.id.clone();
        let _write = self.write_guard.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        self.refuse_secrets(&obs)?;
        let app_key = obs.provenance.first().map(|p| attestation_key(&obs.id, p));
        // The glossary is a read-time fold; nothing materialized depends on this row, so it commits
        // whole with no projection (crash-recovery.md Section 3a).
        self.store.commit_projected(obs, Vec::new(), Vec::new())?;
        self.note_app(app_key.as_deref());
        self.log_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(observation_id)
    }

    /// Projects the workspace type glossary from the observation log (a pure read, Principle 1). Folds every
    /// type_def assertion, keeping the latest definition per (target, name) - last-write-wins by
    /// (ordering HLC, observation id), the cross-node fold order of federation.md Section 4 (M4 Phase 1;
    /// pre-federation observations fall back to a deterministic legacy HLC from observed_at), so the
    /// winner is arrival-order independent (Principle 16) and converges across nodes once stamps exist.
    /// Accumulates a corroboration count (sources) and the representative (highest) trust tier.
    pub fn types(&self, workspace: Option<&str>) -> Result<Vec<TypeDefView>, StoreError> {
        self.types_in(workspace, &ReadCtx::default())
    }

    /// [`Engine::types`] over an existing read context, so a caller that already loaded the
    /// log does not load it again (see [`ReadCtx`]).
    pub(crate) fn types_in(
        &self,
        workspace: Option<&str>,
        cx: &ReadCtx,
    ) -> Result<Vec<TypeDefView>, StoreError> {
        let gates = self.gate_grants(workspace, cx)?;
        // (disc, name) -> (target, description candidates, sources, max effective trust). The
        // description is resolved by the SAME policy as an entity kind (resolution-identity.md
        // Section 6): distinct definitions at a tied top tier are contested, not silently
        // last-write-won (M3a's contested treatment applied to the T-Box - IR5).
        type Acc = (TypeTarget, Vec<BeliefCandidate>, usize, TrustTier);
        // Keyed by (workspace, target, name). Without the workspace this fold merged same-named types
        // from unrelated workspaces into one row - picking one description as the winner and
        // reporting the other as a `contested` competitor, which invents a conflict where there is
        // none and hides one definition behind another. The scoped reads never showed it because they
        // only ever hold one workspace; only the all-workspaces view could, and that is the view the
        // console offers. The merge band already refuses to span workspaces for exactly this reason
        // (P17 clause `p17_candidates_never_span_workspaces_in_the_all_view`); this fold did not.
        let mut descs: BTreeMap<(String, u8, String), Acc> = BTreeMap::new();
        for obs in self.log(workspace, cx)?.iter() {
            let hlc = ordering_hlc(obs);
            let eff = effective_tier(obs, &gates); // receiver-evaluated + gate grants (F13)
            for t in &obs.assertions.type_defs {
                let disc: u8 = match t.target {
                    TypeTarget::Entity => 0,
                    TypeTarget::Relation => 1,
                };
                let e = descs
                    .entry((obs.workspace().to_string(), disc, t.name.clone()))
                    .or_insert_with(|| (t.target, Vec::new(), 0, TrustTier::Unverified));
                e.1.push(BeliefCandidate {
                    value: t.description.clone(),
                    tier: eff,
                    hlc: hlc.clone(),
                    observation: obs.id.clone(),
                });
                e.2 += 1;
                e.3 = e.3.max(eff);
            }
        }
        // Deterministic order by (workspace, target, name) - the BTreeMap key already gives it.
        Ok(descs
            .into_iter()
            .map(|((workspace, _, name), (target, cands, sources, trust))| {
                let (winner, contested, competitors) = self.resolve_kind(Some(&cands));
                let (description, def_source) = match winner {
                    Some((d, obs)) => (d, Some(obs)),
                    None => (String::new(), None),
                };
                TypeDefView {
                    workspace,
                    target,
                    name,
                    description,
                    sources,
                    trust_tier: trust,
                    contested,
                    competitors,
                    def_source,
                }
            })
            .collect())
    }
}
