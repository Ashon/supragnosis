//! supragnosis-sync - the transport-agnostic federation core (M4 Phase 2, docs/federation.md).
//!
//! What lives here: stamping (export-time backfill of the origin's sync metadata), version-vector
//! computation, delta export under selective sharing (F9), event verification (F6), and the apply
//! pipeline (F3: verify -> CAS dedup/absorb -> advance VV). What does NOT live here: transport
//! (HTTPS/TLS/allowlist wire auth is Phase 3) and projection (the engine re-projects; folds are
//! HLC-ordered so re-materialization converges, P16). No IO beyond the injected store port (P20).
//!
//! Stamping model: Phase 2 stamps at the **export boundary** via [`SyncNode::backfill`] - it covers
//! pre-federation attestations and new local attestations uniformly, in deterministic
//! (ordering-HLC, id) order. An attestation without a stamp never leaves the node (F7). The stamp
//! upgrade on `Observation::absorb` (core) makes the write-back an in-place enrichment rather than a
//! duplicated attestation.

#[cfg(feature = "http")]
pub mod http;

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use supragnosis_core::{
    now_millis, observation_content_id, ordering_hlc, verify_attestation, AttestationEvent, Hlc,
    KnowledgeStore, NodeIdentity, Observation, StoreError, SyncMeta, VersionVector,
};

/// Sync-layer failure. Store failures propagate (P5: a backend failure is never an empty result);
/// per-event verification failures are NOT errors - they are rejections in the [`ApplyReport`]
/// (rejecting a forged event is the pipeline working as designed, F6).
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("store failure: {0}")]
    Store(#[from] StoreError),
}

/// Why an inbound event was rejected (F6). Rejections are reported, never silently dropped (P5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    /// The event carries no sync stamp - unstamped attestations never cross the wire (F7).
    Unstamped,
    /// The claimed origin node is not on the receiver's key directory (allowlist/canon binding).
    UnknownOrigin,
    /// The signature does not verify against the claimed origin's key over the recomputed content id.
    BadSignature,
    /// The attestation's workspace does not match the stream's workspace (share-boundary integrity).
    WorkspaceMismatch,
}

/// One rejected event: enough identity to audit without trusting the event's own claims.
#[derive(Debug, Clone)]
pub struct Rejection {
    pub origin_node: String,
    pub origin_seq: u64,
    pub reason: RejectReason,
}

/// Outcome of an apply batch. `accepted` counts events that reached the log (including re-deliveries
/// that deduped into an existing observation - apply is idempotent, F7).
#[derive(Debug, Default)]
pub struct ApplyReport {
    pub accepted: usize,
    pub rejected: Vec<Rejection>,
}

/// Node-local sync state: the node identity plus the HLC clock and per-workspace origin_seq counters.
/// Counters are seeded lazily from the store (max own stamped seq), so a restart continues the dense
/// per-(node, workspace) sequence instead of colliding (F7).
pub struct SyncNode {
    identity: NodeIdentity,
    node_id: String,
    clock: Mutex<Hlc>,
    /// Last origin_seq USED per workspace (next = last + 1). Lazily seeded from the store.
    last_seq: Mutex<HashMap<String, u64>>,
}

impl SyncNode {
    pub fn new(identity: NodeIdentity) -> Self {
        let node_id = identity.node_id();
        Self {
            identity,
            node_id,
            clock: Mutex::new(Hlc::default()),
            last_seq: Mutex::new(HashMap::new()),
        }
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn public_key_hex(&self) -> String {
        self.identity.public_key_hex()
    }

    /// Advance the local HLC for a local event (monotonic, I11).
    fn tick(&self) -> Hlc {
        let mut clock = self.clock.lock().unwrap();
        *clock = Hlc::tick(&clock, now_millis(), &self.node_id);
        clock.clone()
    }

    /// Merge a remote stamp into the local clock (HLC receive rule, I11): after this, everything the
    /// node authors is causally after what it has seen.
    pub fn merge_clock(&self, remote: &Hlc) {
        let mut clock = self.clock.lock().unwrap();
        *clock = Hlc::merge(&clock, remote, now_millis(), &self.node_id);
    }

    /// Next origin_seq for `workspace`, seeding from the store's max own stamped seq on first use.
    fn next_seq(&self, store: &dyn KnowledgeStore, workspace: &str) -> Result<u64, SyncError> {
        let mut map = self.last_seq.lock().unwrap();
        if !map.contains_key(workspace) {
            let mut max_own = 0u64;
            for ev in store.attestations_since(workspace, &VersionVector::default())? {
                if let Some(meta) = &ev.attestation.sync {
                    if meta.origin_node == self.node_id {
                        max_own = max_own.max(meta.origin_seq);
                    }
                }
            }
            map.insert(workspace.to_string(), max_own);
        }
        let last = map.get_mut(workspace).expect("seeded above");
        *last += 1;
        Ok(*last)
    }

    /// Stamps every unstamped attestation in `workspace` with this node's sync metadata - the export
    /// boundary of federation.md Phase 2. Unstamped attestations are locally authored by definition
    /// (inbound events always arrive stamped and are rejected otherwise), so this node IS their
    /// origin. Deterministic order: observations by (ordering HLC, id), attestations by their list
    /// order. Returns the number of attestations stamped.
    pub fn backfill(&self, store: &dyn KnowledgeStore, workspace: &str) -> Result<usize, SyncError> {
        let mut obss = store.all_observations(Some(workspace))?;
        obss.sort_by(|a, b| {
            (ordering_hlc(a), a.id.as_str()).cmp(&(ordering_hlc(b), b.id.as_str()))
        });
        let mut stamped = 0usize;
        for mut obs in obss {
            // Legacy-format rows (stored id != current formula) are never stamped: their signature
            // would bind an id no receiver can recompute - permanently rejected on the wire. They
            // stay local history; `migrate_legacy_ids` re-creates them under the current id.
            if observation_content_id(workspace, &obs.content, &obs.assertions) != obs.id {
                continue;
            }
            let mut changed = false;
            let lineage = obs.derived_from.clone();
            let content_id = obs.id.clone();
            for p in &mut obs.provenance {
                if p.sync.is_some() {
                    continue;
                }
                let mut meta = SyncMeta {
                    origin_node: self.node_id.clone(),
                    origin_seq: self.next_seq(store, workspace)?,
                    hlc: self.tick(),
                    signature: String::new(),
                    // The origin's lineage declaration (signed, F13): what this observation derives
                    // from as known at stamping time.
                    lineage: lineage.clone(),
                };
                meta.signature = self.identity.sign_attestation(&content_id, p, &meta);
                p.sync = Some(meta);
                changed = true;
                stamped += 1;
            }
            if changed {
                // Write-back rides the normal absorb path: the stamped attestation supersedes its
                // unstamped base (the stamp-upgrade rule in core), so this enriches in place instead
                // of duplicating - no bespoke overwrite port needed (P3 stays intact).
                store.add_observation(obs)?;
            }
        }
        Ok(stamped)
    }

    /// The apply pipeline (F3): per event - verify stamp/origin/signature (F6) and workspace
    /// integrity, reconstruct the observation (content id recomputed, never trusted from the wire),
    /// CAS dedup/absorb via the store, advance the version vector, and merge the origin's HLC into
    /// the local clock (I11). Hole-tolerant and idempotent (F7): re-delivery dedups, order does not
    /// matter, and rejection of one event never blocks the rest. The claimed trust tier rides the
    /// attestation verbatim (F13 - evaluation is read-side, never an apply gate).
    pub fn apply(
        &self,
        store: &dyn KnowledgeStore,
        workspace: &str,
        events: Vec<AttestationEvent>,
        origin_keys: &BTreeMap<String, String>,
        vv: &mut VersionVector,
    ) -> Result<ApplyReport, SyncError> {
        let mut report = ApplyReport::default();
        for ev in events {
            match check_event(&ev, workspace, origin_keys) {
                Ok(obs) => {
                    let meta = ev.attestation.sync.as_ref().expect("checked stamped");
                    store.add_observation(obs)?;
                    vv.advance(&meta.origin_node, workspace, meta.origin_seq);
                    self.merge_clock(&meta.hlc);
                    report.accepted += 1;
                }
                Err(reason) => {
                    let (origin_node, origin_seq) = ev
                        .attestation
                        .sync
                        .as_ref()
                        .map(|m| (m.origin_node.clone(), m.origin_seq))
                        .unwrap_or_default();
                    report.rejected.push(Rejection { origin_node, origin_seq, reason });
                }
            }
        }
        Ok(report)
    }
}

/// Verifies one wire event and reconstructs the observation it asserts. The content id is recomputed
/// from (workspace, content, assertions) - a forged id cannot ride the wire - and the signature is
/// checked over that recomputed id with the claimed origin's directory key (F6). The observation's
/// `derived_from` is taken from the SIGNED lineage declaration (F13), not from any unsigned field.
fn check_event(
    ev: &AttestationEvent,
    workspace: &str,
    origin_keys: &BTreeMap<String, String>,
) -> Result<Observation, RejectReason> {
    let Some(meta) = &ev.attestation.sync else {
        return Err(RejectReason::Unstamped);
    };
    if ev.attestation.workspace != workspace {
        return Err(RejectReason::WorkspaceMismatch);
    }
    let Some(pubkey) = origin_keys.get(&meta.origin_node) else {
        return Err(RejectReason::UnknownOrigin);
    };
    let mut obs = Observation::with_assertions(
        ev.content.clone(),
        ev.attestation.clone(),
        ev.assertions.clone(),
    );
    if !verify_attestation(pubkey, &obs.id, &ev.attestation, meta) {
        return Err(RejectReason::BadSignature);
    }
    obs.derived_from = meta.lineage.clone();
    Ok(obs)
}

/// One-shot legacy-id migration (0.x format evolution): re-creates every observation whose stored
/// id predates the current content-address formula under the CURRENT id - content, assertions, and
/// provenance preserved (stale sync stamps stripped: they bound the old id), and the old id appended
/// to `derived_from` so the lineage records the migration. The old row remains local history and
/// never exports (the wire guard in `attestations_since`/`backfill`). Returns the migrated count.
pub fn migrate_legacy_ids(
    store: &dyn KnowledgeStore,
    workspace: &str,
) -> Result<usize, SyncError> {
    let mut migrated = 0usize;
    for obs in store.all_observations(Some(workspace))? {
        let cur_id = observation_content_id(workspace, &obs.content, &obs.assertions);
        if cur_id == obs.id {
            continue;
        }
        // Idempotence: already migrated when the current-id row exists and records the lineage.
        if let Some(existing) = store.get_observation(&cur_id)? {
            if existing.derived_from.contains(&obs.id) {
                continue;
            }
        }
        let mut provs = obs.provenance.clone();
        for p in &mut provs {
            p.sync = None; // stale stamps bound the old id - the new row re-stamps at next export
        }
        if provs.is_empty() {
            continue; // unreachable in practice (P2: at least one attestation), but never panic
        }
        let first = provs.remove(0);
        let mut fresh = Observation::with_assertions(obs.content.clone(), first, obs.assertions.clone());
        for p in provs {
            let mut copy = Observation::with_assertions(obs.content.clone(), p, obs.assertions.clone());
            copy.derived_from = Vec::new();
            fresh.absorb(copy); // union semantics, dedup/order maintained
        }
        fresh.derived_from = obs.derived_from.clone();
        fresh.derived_from.push(obs.id.clone()); // lineage: the migrated row derives from the legacy row
        fresh.derived_from.sort();
        fresh.derived_from.dedup();
        store.add_observation(fresh)?;
        migrated += 1;
    }
    Ok(migrated)
}

/// The delta a node offers a peer for `workspace`: everything stamped that `since` does not cover -
/// but ONLY if the workspace is on the node's outbound share list (selective sharing, F9: filtered
/// before the boundary, an unshared workspace yields nothing rather than an error).
pub fn export_delta(
    store: &dyn KnowledgeStore,
    workspace: &str,
    since: &VersionVector,
    share_workspaces: &[String],
) -> Result<Vec<AttestationEvent>, SyncError> {
    if !share_workspaces.iter().any(|w| w == workspace) {
        return Ok(Vec::new());
    }
    Ok(store.attestations_since(workspace, since)?)
}

/// The node's current version vector for `workspace` (what it holds) - the `advertise` payload.
pub fn version_vector(
    store: &dyn KnowledgeStore,
    workspace: &str,
) -> Result<VersionVector, SyncError> {
    let mut vv = VersionVector::default();
    for ev in store.attestations_since(workspace, &VersionVector::default())? {
        if let Some(meta) = &ev.attestation.sync {
            vv.advance(&meta.origin_node, workspace, meta.origin_seq);
        }
    }
    Ok(vv)
}

#[cfg(test)]
mod tests {
    use super::*;
    use supragnosis_core::Provenance;
    use supragnosis_store::InMemoryStore;

    fn node(seed: u8) -> SyncNode {
        SyncNode::new(NodeIdentity::from_secret_bytes([seed; 32]))
    }

    fn prov(ws: &str, at: u64) -> Provenance {
        Provenance {
            host: "h".into(),
            on_behalf_of: Some("ashon".into()),
            workspace: ws.into(),
            source_ref: None,
            observed_at: at,
            confidence: None,
            trust_tier: Default::default(),
            sync: None,
        }
    }

    fn keys(nodes: &[&SyncNode]) -> BTreeMap<String, String> {
        nodes
            .iter()
            .map(|n| (n.node_id().to_string(), n.public_key_hex()))
            .collect()
    }

    /// Snapshot of a store's log for convergence comparison: id -> (attestation count, sorted
    /// origin/seq pairs, derived_from). Convergence = identical snapshots (F5, log level).
    type LogSnapshot = BTreeMap<String, (usize, Vec<(String, u64)>, Vec<String>)>;
    fn snapshot(store: &InMemoryStore, ws: &str) -> LogSnapshot {
        let mut m = BTreeMap::new();
        for obs in store.all_observations(Some(ws)).unwrap() {
            let mut origins: Vec<(String, u64)> = obs
                .provenance
                .iter()
                .filter_map(|p| p.sync.as_ref().map(|s| (s.origin_node.clone(), s.origin_seq)))
                .collect();
            origins.sort();
            m.insert(obs.id.clone(), (obs.provenance.len(), origins, obs.derived_from.clone()));
        }
        m
    }

    #[test]
    fn backfill_stamps_in_place_without_duplication() {
        let store = InMemoryStore::new();
        let a = node(1);
        let mut o = Observation::new("fact".into(), prov("ws", 10));
        o.derived_from = vec!["parent".into()];
        store.add_observation(o).unwrap();
        store.add_observation(Observation::new("fact two".into(), prov("ws", 20))).unwrap();

        assert_eq!(a.backfill(&store, "ws").unwrap(), 2);
        // Stamped in place: still one attestation per observation, now carrying the stamp + signed lineage.
        for obs in store.all_observations(Some("ws")).unwrap() {
            assert_eq!(obs.provenance.len(), 1, "stamp upgrade must not duplicate attestations");
            let meta = obs.provenance[0].sync.as_ref().expect("stamped");
            assert_eq!(meta.origin_node, a.node_id());
            if obs.content == "fact" {
                assert_eq!(meta.lineage, vec!["parent".to_string()], "lineage declaration signed");
            }
        }
        // Dense seqs 1..=2 in ordering-HLC (observed_at) order; re-backfill is a no-op.
        let vv = version_vector(&store, "ws").unwrap();
        assert_eq!(vv.get(a.node_id(), "ws"), 2);
        assert_eq!(a.backfill(&store, "ws").unwrap(), 0);
    }

    #[test]
    fn seq_continues_after_restart() {
        let store = InMemoryStore::new();
        let a = node(1);
        store.add_observation(Observation::new("one".into(), prov("ws", 1))).unwrap();
        a.backfill(&store, "ws").unwrap();
        // A fresh SyncNode over the same store (process restart) must continue, not collide (F7/F14).
        let a2 = node(1);
        store.add_observation(Observation::new("two".into(), prov("ws", 2))).unwrap();
        a2.backfill(&store, "ws").unwrap();
        let vv = version_vector(&store, "ws").unwrap();
        assert_eq!(vv.get(a2.node_id(), "ws"), 2, "restart continues the dense sequence");
    }

    #[test]
    fn export_respects_share_list_and_vv() {
        let store = InMemoryStore::new();
        let a = node(1);
        store.add_observation(Observation::new("one".into(), prov("ws", 1))).unwrap();
        store.add_observation(Observation::new("two".into(), prov("ws", 2))).unwrap();
        a.backfill(&store, "ws").unwrap();

        // Unshared workspace exports nothing (F9) - filtered before the boundary, not an error.
        assert!(export_delta(&store, "ws", &VersionVector::default(), &[]).unwrap().is_empty());
        let share = vec!["ws".to_string()];
        assert_eq!(export_delta(&store, "ws", &VersionVector::default(), &share).unwrap().len(), 2);
        // A peer that already holds seq 1 receives only the newer event.
        let mut have = VersionVector::default();
        have.advance(a.node_id(), "ws", 1);
        let delta = export_delta(&store, "ws", &have, &share).unwrap();
        assert_eq!(delta.len(), 1);
    }

    #[test]
    fn apply_verifies_rejects_and_stays_idempotent() {
        let store_a = InMemoryStore::new();
        let a = node(1);
        let b = node(2);
        store_a.add_observation(Observation::new("shared fact".into(), prov("ws", 5))).unwrap();
        a.backfill(&store_a, "ws").unwrap();
        let delta = export_delta(&store_a, "ws", &VersionVector::default(), &[String::from("ws")]).unwrap();

        let store_b = InMemoryStore::new();
        let dir = keys(&[&a, &b]);
        let mut vv_b = VersionVector::default();

        // Valid event lands and advances the VV.
        let r = b.apply(&store_b, "ws", delta.clone(), &dir, &mut vv_b).unwrap();
        assert_eq!(r.accepted, 1);
        assert!(r.rejected.is_empty());
        assert!(vv_b.covers(a.node_id(), "ws", 1));

        // Re-delivery dedups (idempotent, F7): same snapshot, still one attestation.
        b.apply(&store_b, "ws", delta.clone(), &dir, &mut vv_b).unwrap();
        assert_eq!(snapshot(&store_b, "ws").len(), 1);
        let only = store_b.all_observations(Some("ws")).unwrap();
        assert_eq!(only[0].provenance.len(), 1, "relay duplicate must not duplicate attestations");

        // Tampered content -> recomputed id differs from the signed one -> BadSignature (F6).
        let mut forged = delta.clone();
        forged[0].content = "poisoned fact".into();
        let r = b.apply(&store_b, "ws", forged, &dir, &mut vv_b).unwrap();
        assert_eq!(r.accepted, 0);
        assert_eq!(r.rejected[0].reason, RejectReason::BadSignature);

        // Unknown origin -> rejected (F6).
        let only_b = keys(&[&b]);
        let r = b.apply(&store_b, "ws", delta.clone(), &only_b, &mut vv_b).unwrap();
        assert_eq!(r.rejected[0].reason, RejectReason::UnknownOrigin);

        // Workspace mismatch -> rejected (share-boundary integrity).
        let r = b.apply(&store_b, "other-ws", delta, &dir, &mut vv_b).unwrap();
        assert_eq!(r.rejected[0].reason, RejectReason::WorkspaceMismatch);
    }

    /// Legacy-format rows (stored id != current formula) never cross the wire; migration re-creates
    /// them under the current id with lineage back to the legacy row, idempotently.
    #[test]
    fn legacy_id_rows_stay_local_and_migrate() {
        let store = InMemoryStore::new();
        let a = node(1);
        let mut legacy = Observation::new("old era fact".into(), prov("ws", 3));
        legacy.id = "legacy-old-formula-id".into(); // simulate a pre-0.1.x id era
        store.add_observation(legacy).unwrap();
        store.add_observation(Observation::new("current fact".into(), prov("ws", 5))).unwrap();

        // Backfill skips the legacy row and export never carries it (wire guard).
        assert_eq!(a.backfill(&store, "ws").unwrap(), 1);
        let share = vec!["ws".to_string()];
        let delta = export_delta(&store, "ws", &VersionVector::default(), &share).unwrap();
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].content, "current fact");

        // Migration re-creates it under the current formula, lineage pointing at the old id.
        assert_eq!(migrate_legacy_ids(&store, "ws").unwrap(), 1);
        assert_eq!(migrate_legacy_ids(&store, "ws").unwrap(), 0, "migration is idempotent");
        let migrated = store
            .all_observations(Some("ws"))
            .unwrap()
            .into_iter()
            .find(|o| o.content == "old era fact" && o.id != "legacy-old-formula-id")
            .expect("migrated row exists under the current id");
        assert!(migrated.derived_from.contains(&"legacy-old-formula-id".to_string()));

        // After migration + backfill the knowledge crosses the wire.
        assert_eq!(a.backfill(&store, "ws").unwrap(), 1);
        let delta = export_delta(&store, "ws", &VersionVector::default(), &share).unwrap();
        assert_eq!(delta.len(), 2, "the migrated row is now exportable");
    }

    /// F5 at the log level: the same event set, delivered in different orders with duplicates and
    /// cross-authored identical content, converges to identical logs and version vectors.
    #[test]
    fn two_nodes_converge_under_any_exchange_order() {
        let share = vec!["ws".to_string()];
        // Build node A with 3 facts and node B with 2 (one content shared with A - CAS dedup case).
        let make = |seed: u8, contents: &[&str]| {
            let store = InMemoryStore::new();
            let n = node(seed);
            for (i, c) in contents.iter().enumerate() {
                store
                    .add_observation(Observation::new((*c).into(), prov("ws", (i as u64 + 1) * 10)))
                    .unwrap();
            }
            n.backfill(&store, "ws").unwrap();
            (store, n)
        };
        let (store_a, a) = make(1, &["alpha", "beta", "shared fact"]);
        let (store_b, b) = make(2, &["gamma", "shared fact"]);
        let dir = keys(&[&a, &b]);

        let delta_a = export_delta(&store_a, "ws", &VersionVector::default(), &share).unwrap();
        let delta_b = export_delta(&store_b, "ws", &VersionVector::default(), &share).unwrap();

        // Three delivery schedules: forward, reversed, and duplicated interleave.
        let schedules: Vec<(Vec<AttestationEvent>, Vec<AttestationEvent>)> = vec![
            (delta_b.clone(), delta_a.clone()),
            (
                delta_b.iter().rev().cloned().collect(),
                delta_a.iter().rev().cloned().collect(),
            ),
            (
                delta_b.iter().chain(delta_b.iter()).cloned().collect(),
                delta_a.iter().chain(delta_a.iter()).cloned().collect(),
            ),
        ];
        let mut snapshots = Vec::new();
        for (to_a, to_b) in schedules {
            // Fresh replicas of each side receive the other's delta under this schedule.
            let (ra, na) = make(1, &["alpha", "beta", "shared fact"]);
            let (rb, nb) = make(2, &["gamma", "shared fact"]);
            let mut vv_a = version_vector(&ra, "ws").unwrap();
            let mut vv_b = version_vector(&rb, "ws").unwrap();
            let r1 = na.apply(&ra, "ws", to_a.clone(), &dir, &mut vv_a).unwrap();
            let r2 = nb.apply(&rb, "ws", to_b.clone(), &dir, &mut vv_b).unwrap();
            assert!(r1.rejected.is_empty() && r2.rejected.is_empty());
            assert_eq!(snapshot(&ra, "ws"), snapshot(&rb, "ws"), "replicas must converge (F5)");
            assert_eq!(vv_a, vv_b, "version vectors must converge");
            snapshots.push(snapshot(&ra, "ws"));
        }
        // Every schedule lands on the same state (order independence, P16).
        assert!(snapshots.windows(2).all(|w| w[0] == w[1]));
        // The cross-authored content deduped by CAS: one observation with both origins attested (F2).
        let merged = snapshots[0]
            .values()
            .find(|(count, _, _)| *count == 2)
            .expect("the shared fact carries both origins");
        assert_eq!(merged.1.len(), 2);
    }
}
