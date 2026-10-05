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
use std::path::PathBuf;
use std::sync::Mutex;

use supragnosis_core::{
    evaluated_tier, now_millis, observation_content_id, ordering_hlc, verify_attestation,
    AssertionStore, AttestationEvent, Hlc, NodeIdentity, Observation, StoreError, SyncMeta,
    VersionVector,
};

/// Sync-layer failure. Store failures propagate (P5: a backend failure is never an empty result);
/// per-event verification failures are NOT errors - they are rejections in the [`ApplyReport`]
/// (rejecting a forged event is the pipeline working as designed, F6).
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("store failure: {0}")]
    Store(#[from] StoreError),
    /// The identity's seq mark (sync-correctness.md Section 5) could not be read or written. Nothing
    /// is stamped without it: a stamp whose seq the mark does not cover could be issued again.
    #[error("seq mark at {0}")]
    SeqMark(String),
}

/// One configured sync server and the credential this node presents to it.
///
/// Per server rather than one token for all of them. A shared bearer hands every configured host the
/// credential that also authorizes at the others, so any one of them could present it elsewhere as
/// this node. That is tolerable while the token only fetches knowledge, and stops being tolerable
/// once a host's own answer decides where knowledge goes - an answer is worth no more than the
/// identification of the caller it was given to (negotiated-surface.md Section 5).
#[derive(Debug, Clone)]
pub struct ServerLink {
    pub url: String,
    pub auth_token: String,
}

/// What a host said this node may reach, and when it said it (federation.md 6e).
///
/// `admits` is `None` while unknown: the host has not answered since this process started, or the
/// last attempt failed. Never an empty vector for that case. An empty grant set is a legitimate
/// answer meaning "admitted, may read nothing" (6a), so collapsing the two would render a host that
/// is down as a grant that was revoked - the reading F12 forbids and F21 clause 4 names.
#[derive(Debug, Clone, Default)]
pub struct NegotiatedSurface {
    pub admits: Option<Vec<String>>,
    pub negotiated_at: Option<u64>,
}

/// Per-server negotiated surfaces, keyed by server url.
///
/// Link-local runtime state. It is never written to the observation log, so no projection and no
/// proposition depends on it (F21 clause 3) - and the writer is the daemon's background health loop
/// alone, so reading it costs a lock and never a round trip (F21 clause 6, N1).
pub type NegotiatedSurfaces =
    std::sync::Arc<std::sync::RwLock<std::collections::BTreeMap<String, NegotiatedSurface>>>;

/// Record what a host's health check found, as the negotiated surface for that link.
///
/// `Some(admits)` is an answer and gets a timestamp; `None` means the check failed, and the entry
/// becomes **unknown** - not an empty grant set (F21 clause 4). The two are different facts: an
/// empty list is a host saying "admitted, may read nothing" (6a), while unknown is a host that has
/// not said anything, and collapsing them renders a host that is down as a grant that was revoked
/// (F12). The timestamp goes with the answer and only with it, so a reader can always tell how old
/// what it is acting on is and never mistakes a stale grant for a current one (F21 clause 6).
pub fn record_ping(surfaces: &NegotiatedSurfaces, url: &str, admits: Option<Vec<String>>, at: u64) {
    let entry = match admits {
        Some(list) => NegotiatedSurface { admits: Some(list), negotiated_at: Some(at) },
        None => NegotiatedSurface::default(),
    };
    if let Ok(mut m) = surfaces.write() {
        m.insert(url.to_string(), entry);
    }
}

/// Which servers a workspace's round should reach, and which were left out because they said so.
#[derive(Debug, Clone, Default)]
pub struct Routed {
    /// Server urls to consult, in the order they were configured.
    pub consult: Vec<String>,
    /// Server urls skipped because their negotiated surface does not admit this workspace.
    pub skipped: Vec<String>,
}

/// Narrow a round to the hosts that admit the workspace (federation.md 6e, F21 clause 2).
///
/// **A host is skipped only when it said so.** `admits: None` means unknown - the host has not
/// answered since this process started, or its last check failed - and skipping on that would turn
/// "not asked yet" into "not allowed", which is the absence-as-negation reading P5 exists to
/// prevent and the empty-versus-unknown collapse F12 forbids. So unknown is consulted, and the
/// host's own `403` remains the answer if it really does not admit the workspace: the loud failure
/// stays reachable rather than being replaced by a quiet skip.
///
/// Narrowing only. The map can remove a host from a round; nothing here can add a workspace to what
/// this node shares. A host's answer is its claim about access, and letting it widen local policy
/// would turn a read authorization into a write authorization - P18 on the outbound axis.
pub fn route(links: &[ServerLink], surfaces: &NegotiatedSurfaces, workspace: &str) -> Routed {
    let map = surfaces.read().ok();
    let mut r = Routed::default();
    for link in links {
        let admits = map.as_ref().and_then(|m| m.get(&link.url)).and_then(|s| s.admits.as_ref());
        match admits {
            Some(list) if !list.iter().any(|w| w == workspace) => r.skipped.push(link.url.clone()),
            _ => r.consult.push(link.url.clone()),
        }
    }
    r
}

/// The disagreement between what this node shares and what a host admits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SurfaceDiff {
    /// Shared here and admitted there.
    pub both: Vec<String>,
    /// This node lists it, the host does not admit it - a setup error.
    pub local_only: Vec<String>,
    /// The host would admit it, this node does not share it - knowledge left on the table.
    pub peer_only: Vec<String>,
}

/// Three buckets rather than an intersection: intersecting silently hides the misconfiguration that
/// produced the gap, and the gap is the whole operator value (N6). Each bucket is sorted, so a
/// response over the same state is reproducible down to its order (P16).
pub fn surface_diff(local_share: &[String], admits: &[String]) -> SurfaceDiff {
    let mut d = SurfaceDiff::default();
    for w in local_share {
        if admits.contains(w) {
            d.both.push(w.clone());
        } else {
            d.local_only.push(w.clone());
        }
    }
    for w in admits {
        if !local_share.contains(w) {
            d.peer_only.push(w.clone());
        }
    }
    for b in [&mut d.both, &mut d.local_only, &mut d.peer_only] {
        b.sort();
        b.dedup();
    }
    d
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
    /// The event is signed by a known origin but its content is not well-formed (out-of-range
    /// confidence, empty-referent assertion). A signature proves origin, not well-formedness (P18),
    /// so a malformed peer event is refused before it reaches the permanent log (P1/P2).
    Malformed(String),
    /// This release cannot decode the event - an enum value or a shape from a newer one
    /// (sync-correctness.md Section 6). Rejected alone; the rest of its batch proceeds.
    Undecodable(String),
    /// An earlier event of the same (origin, workspace) stream, at this seq, was rejected in this
    /// batch, so this one is not applied either (sync-correctness.md Section 6). The receiver's
    /// version vector then stays below the rejected event, which is offered again every round until
    /// it is accepted - a held stream, never a permanent hole.
    Held(u64),
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
/// Counters are seeded lazily from the store (max own stamped seq) and from the identity's seq mark,
/// so neither a restart nor a store restored from an older backup issues a seq twice (F7,
/// sync-correctness.md Section 5).
pub struct SyncNode {
    identity: NodeIdentity,
    node_id: String,
    clock: Mutex<Hlc>,
    /// Last origin_seq USED per workspace (next = last + 1). Lazily seeded from the store and mark.
    last_seq: Mutex<HashMap<String, u64>>,
    /// Held from a backfill's snapshot to its last write, so two callers sharing this node cannot
    /// both stamp one unstamped attestation (sync-correctness.md Section 4).
    backfill: Mutex<()>,
    /// Where the identity's seq mark lives - beside the key it belongs to. `None` keeps the counter
    /// store-seeded only (tests, and nodes without a key file).
    seq_mark: Option<PathBuf>,
}

impl SyncNode {
    pub fn new(identity: NodeIdentity) -> Self {
        let node_id = identity.node_id();
        Self {
            identity,
            node_id,
            clock: Mutex::new(Hlc::default()),
            last_seq: Mutex::new(HashMap::new()),
            backfill: Mutex::new(()),
            seq_mark: None,
        }
    }

    /// Keeps the seq high-water mark in `path`, beside the node key (sync-correctness.md Section 5).
    /// The counter then lives with the identity: a store restored from an older backup continues
    /// past every seq the identity issued, instead of reissuing them for different events.
    pub fn with_seq_mark(mut self, path: PathBuf) -> Self {
        self.seq_mark = Some(path);
        self
    }

    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    pub fn public_key_hex(&self) -> String {
        self.identity.public_key_hex()
    }

    /// Merge a remote stamp into the local clock (HLC receive rule, I11): after this, everything the
    /// node authors is causally after what it has seen.
    pub fn merge_clock(&self, remote: &Hlc) {
        let mut clock = self.clock.lock().unwrap();
        *clock = Hlc::merge(&clock, remote, now_millis(), &self.node_id);
    }

    /// The seq mark: per workspace, the highest seq this identity has reserved. Absent is empty.
    fn read_mark(&self) -> Result<BTreeMap<String, u64>, SyncError> {
        let Some(path) = &self.seq_mark else {
            return Ok(BTreeMap::new());
        };
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| SyncError::SeqMark(format!("{}: unreadable ({e})", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(SyncError::SeqMark(format!("{}: {e}", path.display()))),
        }
    }

    /// Writes the mark durably - a temporary file synced, then renamed over the old one - so a
    /// crash leaves either mark whole.
    fn write_mark(&self, mark: &BTreeMap<String, u64>) -> Result<(), SyncError> {
        let Some(path) = &self.seq_mark else {
            return Ok(());
        };
        let fail = |e: std::io::Error| SyncError::SeqMark(format!("{}: {e}", path.display()));
        let tmp = path.with_extension("seq.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).map_err(fail)?;
            f.write_all(serde_json::to_string(mark).expect("a map serializes").as_bytes())
                .map_err(fail)?;
            f.sync_all().map_err(fail)?;
        }
        std::fs::rename(&tmp, path).map_err(fail)
    }

    /// The last seq used in `workspace`, seeded on first use from the greater of the store's own
    /// stamps and the identity's mark.
    fn seeded<'a>(
        &self,
        map: &'a mut HashMap<String, u64>,
        store: &dyn AssertionStore,
        workspace: &str,
    ) -> Result<&'a mut u64, SyncError> {
        if !map.contains_key(workspace) {
            let mut max_own = self.read_mark()?.get(workspace).copied().unwrap_or(0);
            for ev in store.attestations_since(workspace, &VersionVector::default())? {
                if let Some(meta) = &ev.attestation.sync {
                    if meta.origin_node == self.node_id {
                        max_own = max_own.max(meta.origin_seq);
                    }
                }
            }
            map.insert(workspace.to_string(), max_own);
        }
        Ok(map.get_mut(workspace).expect("seeded above"))
    }

    /// Reserves `n` consecutive seqs in `workspace` and returns the first. The mark is raised and
    /// written before any of them is used, so a crash can leave a hole (harmless, F7) but never a
    /// seq that is issued again.
    fn reserve(
        &self,
        store: &dyn AssertionStore,
        workspace: &str,
        n: u64,
    ) -> Result<u64, SyncError> {
        let mut map = self.last_seq.lock().unwrap();
        let last = self.seeded(&mut map, store, workspace)?;
        let first = *last + 1;
        let reserved = *last + n;
        if self.seq_mark.is_some() {
            let mut mark = self.read_mark()?;
            let entry = mark.entry(workspace.to_string()).or_default();
            *entry = (*entry).max(reserved);
            self.write_mark(&mark)?;
        }
        *last = reserved;
        Ok(first)
    }

    /// Raises the counter to at least `seen`: a seq of this node's own stream that another node
    /// already holds - a hub's advertised version vector, or an own event pulled back. A node whose
    /// store lost stamps then continues past them instead of issuing them again (Section 5).
    pub fn floor_seq(
        &self,
        store: &dyn AssertionStore,
        workspace: &str,
        seen: u64,
    ) -> Result<(), SyncError> {
        let mut map = self.last_seq.lock().unwrap();
        let last = self.seeded(&mut map, store, workspace)?;
        *last = (*last).max(seen);
        Ok(())
    }

    /// Stamps every unstamped attestation in `workspace` with this node's sync metadata - the export
    /// boundary of federation.md Phase 2. Unstamped attestations are locally authored by definition
    /// (inbound events always arrive stamped and are rejected otherwise), so this node IS their
    /// origin. Deterministic order: observations by (ordering HLC, id), attestations by their list
    /// order. Returns the number of attestations stamped.
    pub fn backfill(
        &self,
        store: &dyn AssertionStore,
        workspace: &str,
    ) -> Result<usize, SyncError> {
        // One backfill at a time per node, from snapshot to last write (Section 4). Seqs are then
        // allocated and committed in the same ascending order, and a waiting caller finds the rows
        // already stamped.
        let _one_at_a_time = self.backfill.lock().unwrap();
        let mut obss = store.all_observations(Some(workspace))?;
        obss.sort_by(|a, b| {
            (ordering_hlc(a), a.id.as_str()).cmp(&(ordering_hlc(b), b.id.as_str()))
        });
        // Legacy-format rows (stored id != current formula) are never stamped: their signature
        // would bind an id no receiver can recompute - permanently rejected on the wire. They stay
        // local history; `migrate_legacy_ids` re-creates them under the current id.
        obss.retain(|o| observation_content_id(workspace, &o.content, &o.assertions) == o.id);
        let needed =
            obss.iter().flat_map(|o| &o.provenance).filter(|p| p.sync.is_none()).count() as u64;
        if needed == 0 {
            return Ok(0);
        }
        let mut seq = self.reserve(store, workspace, needed)?;
        let mut prev: Option<Hlc> = None;
        let mut stamped = 0usize;
        for mut obs in obss {
            let mut changed = false;
            let lineage = obs.derived_from.clone();
            let content_id = obs.id.clone();
            for p in &mut obs.provenance {
                if p.sync.is_some() {
                    continue;
                }
                let hlc = self.authored_hlc(p.observed_at, prev.as_ref());
                prev = Some(hlc.clone());
                let mut meta = SyncMeta {
                    origin_node: self.node_id.clone(),
                    origin_seq: seq,
                    hlc,
                    signature: String::new(),
                    // The origin's lineage declaration (signed, F13): what this observation derives
                    // from as known at stamping time.
                    lineage: lineage.clone(),
                };
                seq += 1;
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

    /// The HLC of an attestation authored at `observed_at` (sync-correctness.md Section 3): the time
    /// it was observed, not the time it is exported. `counter` separates this node's stamps that
    /// share a wall within one pass, and the clock merges the stamp so later local events still
    /// order after it. Before stamping, the row ordered by `Hlc::legacy(observed_at)`, so stamping
    /// reorders nothing on the node that stamps.
    fn authored_hlc(&self, observed_at: u64, prev: Option<&Hlc>) -> Hlc {
        let counter = match prev {
            Some(p) if p.wall == observed_at => p.counter + 1,
            _ => 0,
        };
        let hlc = Hlc { wall: observed_at, counter, node: self.node_id.clone() };
        let mut clock = self.clock.lock().unwrap();
        if hlc > *clock {
            *clock = hlc.clone();
        }
        hlc
    }

    /// The apply pipeline (F3): per event - verify stamp/origin/signature (F6) and workspace
    /// integrity, reconstruct the observation (content id recomputed, never trusted from the wire),
    /// CAS dedup/absorb via the store, advance the version vector, and merge the origin's HLC into
    /// the local clock (I11). Idempotent, and tolerant of holes no event fills (F7). The claimed
    /// trust tier rides the attestation verbatim (F13 - evaluation is read-side, never an apply gate).
    ///
    /// Events are taken in (origin, seq) order, and the first rejection in a stream holds the rest of
    /// that stream for this batch (sync-correctness.md Section 6): the version vector then stays
    /// below the rejected event, so it is offered again until it is accepted, instead of being
    /// skipped for good by a later seq. Other streams carry on.
    pub fn apply(
        &self,
        store: &dyn AssertionStore,
        workspace: &str,
        events: Vec<AttestationEvent>,
        origin_keys: &BTreeMap<String, String>,
        vv: &mut VersionVector,
    ) -> Result<ApplyReport, SyncError> {
        let inbound = events.into_iter().map(|e| Inbound::Decoded(Box::new(e))).collect();
        self.apply_inbound(store, workspace, inbound, origin_keys, vv)
    }

    /// [`Self::apply`] over events as they came off the wire. Each is decoded on its own, so one
    /// this release cannot read - an enum value from a newer one - is rejected as `Undecodable` and
    /// holds only its own stream, where decoding the batch whole would fail every event in it.
    pub fn apply_wire(
        &self,
        store: &dyn AssertionStore,
        workspace: &str,
        events: Vec<serde_json::Value>,
        origin_keys: &BTreeMap<String, String>,
        vv: &mut VersionVector,
    ) -> Result<ApplyReport, SyncError> {
        let inbound = events.into_iter().map(Inbound::decode).collect();
        self.apply_inbound(store, workspace, inbound, origin_keys, vv)
    }

    fn apply_inbound(
        &self,
        store: &dyn AssertionStore,
        workspace: &str,
        mut inbound: Vec<Inbound>,
        origin_keys: &BTreeMap<String, String>,
        vv: &mut VersionVector,
    ) -> Result<ApplyReport, SyncError> {
        inbound.sort_by_cached_key(Inbound::stream);
        let mut report = ApplyReport::default();
        let mut held: HashMap<String, u64> = HashMap::new();
        for item in inbound {
            let (origin_node, origin_seq) = item.stream();
            if let Some(at) = held.get(&origin_node) {
                let reason = RejectReason::Held(*at);
                report.rejected.push(Rejection { origin_node, origin_seq, reason });
                continue;
            }
            let checked = match item {
                Inbound::Decoded(ev) => check_event(&ev, workspace, origin_keys).map(|o| (o, *ev)),
                Inbound::Undecodable { error, .. } => Err(RejectReason::Undecodable(error)),
            };
            match checked {
                Ok((obs, ev)) => {
                    let meta = ev.attestation.sync.as_ref().expect("checked stamped");
                    store.add_observation(obs)?;
                    vv.advance(&meta.origin_node, workspace, meta.origin_seq);
                    self.merge_clock(&meta.hlc);
                    if meta.origin_node == self.node_id {
                        // An own event pulled back: the counter continues past it (Section 5).
                        self.floor_seq(store, workspace, meta.origin_seq)?;
                    }
                    report.accepted += 1;
                }
                Err(reason) => {
                    // An unstamped event has no stream to hold.
                    if !origin_node.is_empty() {
                        held.insert(origin_node.clone(), origin_seq);
                    }
                    report.rejected.push(Rejection { origin_node, origin_seq, reason });
                }
            }
        }
        Ok(report)
    }
}

/// One inbound event, decoded or not (sync-correctness.md Section 6).
enum Inbound {
    Decoded(Box<AttestationEvent>),
    /// Not decodable by this release. Its stream is read from the raw JSON where it can be, so the
    /// stream is held like any other rejection.
    Undecodable {
        origin_node: String,
        origin_seq: u64,
        error: String,
    },
}

impl Inbound {
    fn decode(raw: serde_json::Value) -> Inbound {
        let sync = &raw["attestation"]["sync"];
        let origin_node = sync["origin_node"].as_str().unwrap_or_default().to_string();
        let origin_seq = sync["origin_seq"].as_u64().unwrap_or_default();
        match serde_json::from_value::<AttestationEvent>(raw) {
            Ok(ev) => Inbound::Decoded(Box::new(ev)),
            Err(e) => Inbound::Undecodable { origin_node, origin_seq, error: e.to_string() },
        }
    }

    /// The (origin, seq) an event belongs to; unstamped events sort first, under the empty origin.
    fn stream(&self) -> (String, u64) {
        match self {
            Inbound::Decoded(ev) => ev
                .attestation
                .sync
                .as_ref()
                .map(|m| (m.origin_node.clone(), m.origin_seq))
                .unwrap_or_default(),
            Inbound::Undecodable { origin_node, origin_seq, .. } => {
                (origin_node.clone(), *origin_seq)
            }
        }
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
    // A valid signature proves the origin, not that the content is well-formed (P18). Refuse a
    // signed-but-malformed event before it reaches the permanent log - the local observe path refuses
    // the same shape (P1 well-formedness, P2 confidence range), and F7 allows per-event rejection.
    obs.check_well_formed().map_err(RejectReason::Malformed)?;
    obs.derived_from = meta.lineage.clone();
    Ok(obs)
}

/// One-shot legacy-id migration (0.x format evolution): re-creates every observation whose stored
/// id predates the current content-address formula under the CURRENT id - content, assertions, and
/// provenance preserved (stale sync stamps stripped: they bound the old id), and the old id appended
/// to `derived_from` so the lineage records the migration. The old row remains local history and
/// never exports (the wire guard in `attestations_since`/`backfill`). Returns the migrated count.
pub fn migrate_legacy_ids(store: &dyn AssertionStore, workspace: &str) -> Result<usize, SyncError> {
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
            // Clamp BEFORE the stamp drops: `evaluated_tier` trusts a stamp-less claim at face
            // value, so a synced claim carried verbatim would evaluate above HostSigned after
            // the migration (P18 - same rule as `rekey_workspace`).
            p.trust_tier = evaluated_tier(p);
            p.sync = None; // stale stamps bound the old id - the new row re-stamps at next export
        }
        if provs.is_empty() {
            continue; // unreachable in practice (P2: at least one attestation), but never panic
        }
        let first = provs.remove(0);
        let mut fresh =
            Observation::with_assertions(obs.content.clone(), first, obs.assertions.clone());
        for p in provs {
            let mut copy =
                Observation::with_assertions(obs.content.clone(), p, obs.assertions.clone());
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
    store: &dyn AssertionStore,
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
    store: &dyn AssertionStore,
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
        nodes.iter().map(|n| (n.node_id().to_string(), n.public_key_hex())).collect()
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

    fn stamps(store: &InMemoryStore, ws: &str) -> Vec<SyncMeta> {
        let mut out: Vec<SyncMeta> = store
            .all_observations(Some(ws))
            .unwrap()
            .into_iter()
            .flat_map(|o| o.provenance.into_iter().filter_map(|p| p.sync))
            .collect();
        out.sort_by_key(|m| (m.origin_node.clone(), m.origin_seq));
        out
    }

    fn tmp_mark() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("supragnosis-seq-{}-{nanos}.json", std::process::id()))
    }

    /// sync-correctness.md Section 3 (D1): a stamp carries when the attestation was observed, not
    /// when it was exported. Node A observed X at t=100 and synced nothing; it has since seen B's
    /// edit, stamped at t=500, so its clock is far past both. Exporting X must not make it the
    /// newer edit.
    #[test]
    fn a_stamp_carries_the_authoring_time_not_the_export_time() {
        let (a, b) = (node(1), node(2));
        let (store_a, store_b) = (InMemoryStore::new(), InMemoryStore::new());
        store_a.add_observation(Observation::new("X".into(), prov("ws", 100))).unwrap();
        store_b.add_observation(Observation::new("Y".into(), prov("ws", 500))).unwrap();
        b.backfill(&store_b, "ws").unwrap();
        let y = store_b.attestations_since("ws", &VersionVector::default()).unwrap();
        a.apply(&store_a, "ws", y, &keys(&[&b]), &mut VersionVector::default()).unwrap();
        a.merge_clock(&Hlc { wall: 1_000_000, counter: 0, node: "elsewhere".into() });

        a.backfill(&store_a, "ws").unwrap();
        let mine = stamps(&store_a, "ws");
        let x = mine.iter().find(|m| m.origin_node == a.node_id()).expect("X stamped");
        let y = mine.iter().find(|m| m.origin_node == b.node_id()).expect("Y held");
        assert_eq!(x.hlc.wall, 100, "the authoring time, not the export time");
        assert!(x.hlc < y.hlc, "the older edit stays older after it is exported");
    }

    /// Section 4 (D2): two callers sharing one node backfill at once, as two MCP sessions or two
    /// peers' pulls at a hub do. Each attestation is stamped once, under consecutive seqs.
    #[test]
    fn two_backfills_at_once_stamp_each_attestation_once() {
        let n = std::sync::Arc::new(node(3));
        let store = std::sync::Arc::new(InMemoryStore::new());
        for i in 0..300 {
            store
                .add_observation(Observation::new(format!("fact {i}"), prov("ws", i)))
                .unwrap();
        }
        std::thread::scope(|scope| {
            for _ in 0..2 {
                let (n, store) = (n.clone(), store.clone());
                scope.spawn(move || n.backfill(store.as_ref(), "ws").unwrap());
            }
        });
        for obs in store.all_observations(Some("ws")).unwrap() {
            assert_eq!(obs.provenance.len(), 1, "one act, one attestation: {}", obs.id);
        }
        let seqs: Vec<u64> = stamps(&store, "ws").iter().map(|m| m.origin_seq).collect();
        assert_eq!(seqs, (1..=300).collect::<Vec<_>>());
    }

    /// Section 5 (D3): the counter lives with the identity. A store restored from a backup taken
    /// before seqs 1-3 were issued continues at 4, instead of issuing 1 again for a different event
    /// that every peer's version vector already covers.
    #[test]
    fn a_restored_store_does_not_reissue_a_seq() {
        let mark = tmp_mark();
        let id = || NodeIdentity::from_secret_bytes([4u8; 32]);
        let before = InMemoryStore::new();
        for i in 0..3 {
            before
                .add_observation(Observation::new(format!("sent {i}"), prov("ws", i)))
                .unwrap();
        }
        SyncNode::new(id()).with_seq_mark(mark.clone()).backfill(&before, "ws").unwrap();

        let restored = InMemoryStore::new();
        restored.add_observation(Observation::new("new".into(), prov("ws", 9))).unwrap();
        SyncNode::new(id())
            .with_seq_mark(mark.clone())
            .backfill(&restored, "ws")
            .unwrap();
        assert_eq!(stamps(&restored, "ws")[0].origin_seq, 4);
        let _ = std::fs::remove_file(mark);
    }

    /// Section 5, without a mark: what a host holds of this node's own stream floors the counter,
    /// whether it is advertised or pulled back.
    #[test]
    fn what_a_peer_holds_of_this_nodes_stream_floors_its_counter() {
        let n = node(5);
        let advertised = InMemoryStore::new();
        n.floor_seq(&advertised, "ws", 7).unwrap();
        advertised
            .add_observation(Observation::new("after".into(), prov("ws", 1)))
            .unwrap();
        n.backfill(&advertised, "ws").unwrap();
        assert_eq!(stamps(&advertised, "ws")[0].origin_seq, 8, "advertised by a hub");

        let sent = InMemoryStore::new();
        for i in 0..2 {
            sent.add_observation(Observation::new(format!("sent {i}"), prov("ws", i)))
                .unwrap();
        }
        n.backfill(&sent, "ws").unwrap();
        let back = sent.attestations_since("ws", &VersionVector::default()).unwrap();
        let fresh = node(5);
        let restored = InMemoryStore::new();
        fresh
            .apply(&restored, "ws", back, &keys(&[&n]), &mut VersionVector::default())
            .unwrap();
        restored
            .add_observation(Observation::new("new".into(), prov("ws", 50)))
            .unwrap();
        fresh.backfill(&restored, "ws").unwrap();
        let own: Vec<u64> = stamps(&restored, "ws").iter().map(|m| m.origin_seq).collect();
        // n had issued 8 above, so its sent events are 9 and 10; the restored node continues at 11.
        assert_eq!(own, vec![9, 10, 11], "pulled back, then continued past them");
    }

    /// Section 6 (D4): a rejected event holds its stream for the batch, so the receiver's version
    /// vector stays below it. It is offered again, and once it is accepted the stream fills in -
    /// where advancing past it would have skipped it for good.
    #[test]
    fn a_rejected_event_holds_its_stream_until_it_is_accepted() {
        let (o, r) = (node(6), node(7));
        let (store_o, store_r) = (InMemoryStore::new(), InMemoryStore::new());
        for i in 0..3 {
            store_o
                .add_observation(Observation::new(format!("e{i}"), prov("ws", i)))
                .unwrap();
        }
        o.backfill(&store_o, "ws").unwrap();
        let mut good = store_o.attestations_since("ws", &VersionVector::default()).unwrap();
        good.sort_by_key(|e| e.attestation.sync.as_ref().unwrap().origin_seq);
        let mut bad = good.clone();
        bad[1].attestation.sync.as_mut().unwrap().signature = "00".repeat(64);

        let report = r
            .apply(&store_r, "ws", bad, &keys(&[&o]), &mut VersionVector::default())
            .unwrap();
        assert_eq!(report.accepted, 1);
        let reasons: Vec<_> = report.rejected.iter().map(|x| x.reason.clone()).collect();
        assert_eq!(reasons, vec![RejectReason::BadSignature, RejectReason::Held(2)]);
        assert_eq!(version_vector(&store_r, "ws").unwrap().get(o.node_id(), "ws"), 1);

        let since = version_vector(&store_r, "ws").unwrap();
        let offered = store_o.attestations_since("ws", &since).unwrap();
        assert_eq!(offered.len(), 2, "the rejected event and the one held behind it");
        let report = r
            .apply(&store_r, "ws", offered, &keys(&[&o]), &mut VersionVector::default())
            .unwrap();
        assert_eq!((report.accepted, report.rejected.len()), (2, 0));
    }

    /// Section 6 (D10): an event this release cannot decode is rejected alone and holds only its
    /// own stream; the rest of the batch is applied. A typed batch would fail every event in it.
    #[test]
    fn an_event_this_release_cannot_decode_is_rejected_alone() {
        let (o, p, r) = (node(8), node(9), node(10));
        let (store_o, store_p, store_r) =
            (InMemoryStore::new(), InMemoryStore::new(), InMemoryStore::new());
        store_o
            .add_observation(Observation::new("readable".into(), prov("ws", 1)))
            .unwrap();
        store_p
            .add_observation(Observation::new("from later".into(), prov("ws", 2)))
            .unwrap();
        o.backfill(&store_o, "ws").unwrap();
        p.backfill(&store_p, "ws").unwrap();
        let mut wire: Vec<serde_json::Value> = store_o
            .attestations_since("ws", &VersionVector::default())
            .unwrap()
            .into_iter()
            .chain(store_p.attestations_since("ws", &VersionVector::default()).unwrap())
            .map(|e| serde_json::to_value(e).unwrap())
            .collect();
        let later = wire
            .iter_mut()
            .find(|v| v["attestation"]["sync"]["origin_node"] == p.node_id())
            .unwrap();
        later["attestation"]["trust_tier"] = "a_tier_from_a_later_release".into();

        let report = r
            .apply_wire(&store_r, "ws", wire, &keys(&[&o, &p]), &mut VersionVector::default())
            .unwrap();
        assert_eq!(report.accepted, 1, "the readable event lands");
        assert_eq!(report.rejected.len(), 1);
        assert_eq!(report.rejected[0].origin_node, p.node_id());
        assert!(matches!(report.rejected[0].reason, RejectReason::Undecodable(_)));
    }

    /// A failed check writes unknown, never an empty grant - and only an answer carries a time.
    #[test]
    fn a_failed_check_records_unknown_and_not_an_empty_grant() {
        let surfaces: NegotiatedSurfaces = Default::default();
        let url = "https://hub";

        // An answer: the list is kept and stamped with when it arrived.
        record_ping(&surfaces, url, Some(vec!["ws".into()]), 111);
        {
            let m = surfaces.read().unwrap();
            let e = m.get(url).expect("entry");
            assert_eq!(e.admits.as_deref(), Some(&["ws".to_string()][..]));
            assert_eq!(e.negotiated_at, Some(111), "an answer carries its time (F21 clause 6)");
        }

        // An empty list is also an answer - "admitted, may read nothing" - and stays distinguishable
        // from not knowing.
        record_ping(&surfaces, url, Some(vec![]), 222);
        {
            let m = surfaces.read().unwrap();
            let e = m.get(url).expect("entry");
            assert_eq!(e.admits.as_deref(), Some(&[][..]), "empty is an answer, not an absence");
            assert_eq!(e.negotiated_at, Some(222));
        }

        // A failed check erases the answer rather than emptying it: a host that is down must not
        // read as one that revoked everything (F21 clause 4, F12).
        record_ping(&surfaces, url, None, 333);
        {
            let m = surfaces.read().unwrap();
            let e = m.get(url).expect("entry");
            assert!(e.admits.is_none(), "unreachable is unknown, never an empty grant set");
            assert!(e.negotiated_at.is_none(), "there is no answer to date");
        }
        // And unknown does not narrow, while the empty answer did.
        assert_eq!(
            route(&[ServerLink { url: url.into(), auth_token: "t".into() }], &surfaces, "ws")
                .skipped
                .len(),
            0
        );
    }

    /// Routing skips a host only on an explicit refusal, never on not knowing.
    #[test]
    fn routing_narrows_on_a_refusal_and_never_on_ignorance() {
        let link = |u: &str| ServerLink { url: u.into(), auth_token: "t".into() };
        let links = vec![link("https://a"), link("https://b"), link("https://c")];
        let surfaces: NegotiatedSurfaces = Default::default();
        {
            let mut m = surfaces.write().unwrap();
            // a admits it, b answered and does not, c has never answered.
            m.insert(
                "https://a".into(),
                NegotiatedSurface { admits: Some(vec!["ws".into()]), negotiated_at: Some(1) },
            );
            m.insert(
                "https://b".into(),
                NegotiatedSurface { admits: Some(vec!["other".into()]), negotiated_at: Some(1) },
            );
        }
        let r = route(&links, &surfaces, "ws");
        assert_eq!(r.consult, ["https://a", "https://c"], "unknown is consulted, not skipped");
        assert_eq!(r.skipped, ["https://b"], "only the host that said no is skipped");

        // An empty grant set is an answer - "admitted, may read nothing" - so it does narrow.
        {
            let mut m = surfaces.write().unwrap();
            m.insert(
                "https://c".into(),
                NegotiatedSurface { admits: Some(vec![]), negotiated_at: Some(1) },
            );
        }
        let r = route(&links, &surfaces, "ws");
        assert_eq!(r.consult, ["https://a"]);
        assert_eq!(r.skipped, ["https://b", "https://c"]);
    }

    /// The three buckets, and the reason there are three. An intersection would report the first
    /// bucket and drop the two that say something is wrong.
    #[test]
    fn the_surface_difference_reports_both_directions_of_disagreement() {
        let local = ["cloud".to_string(), "network".to_string()];
        let admits = ["network".to_string(), "rebellions".to_string()];
        let d = surface_diff(&local, &admits);
        assert_eq!(d.both, ["network"], "shared here and admitted there");
        assert_eq!(d.local_only, ["cloud"], "listed here, not admitted - a setup error");
        assert_eq!(d.peer_only, ["rebellions"], "admitted there, not shared - left on the table");

        // An empty grant set is an answer, not an absence: everything local becomes local_only
        // rather than the difference reporting nothing.
        let d = surface_diff(&local, &[]);
        assert_eq!(d.local_only, ["cloud", "network"]);
        assert!(d.both.is_empty() && d.peer_only.is_empty());

        // Order in, order out: the buckets are sorted so a response is reproducible (P16).
        let d = surface_diff(&["b".into(), "a".into()], &["b".into(), "a".into()]);
        assert_eq!(d.both, ["a", "b"]);
    }

    #[test]
    fn backfill_stamps_in_place_without_duplication() {
        let store = InMemoryStore::new();
        let a = node(1);
        let mut o = Observation::new("fact".into(), prov("ws", 10));
        o.derived_from = vec!["parent".into()];
        store.add_observation(o).unwrap();
        store
            .add_observation(Observation::new("fact two".into(), prov("ws", 20)))
            .unwrap();

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
        store_a
            .add_observation(Observation::new("shared fact".into(), prov("ws", 5)))
            .unwrap();
        a.backfill(&store_a, "ws").unwrap();
        let delta =
            export_delta(&store_a, "ws", &VersionVector::default(), &[String::from("ws")]).unwrap();

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

    /// A signed-but-malformed peer event (out-of-range confidence) is refused before the log
    /// (P18: a signature proves origin, not well-formedness; P1/P2 gate every ingest surface).
    #[test]
    fn apply_rejects_signed_but_malformed_event() {
        let store_a = InMemoryStore::new();
        let a = node(1);
        let b = node(2);
        let mut bad_prov = prov("ws", 7);
        bad_prov.confidence = Some(5.0); // out of [0.0, 1.0]
        store_a
            .add_observation(Observation::new("over-confident".into(), bad_prov))
            .unwrap();
        a.backfill(&store_a, "ws").unwrap();
        let delta =
            export_delta(&store_a, "ws", &VersionVector::default(), &[String::from("ws")]).unwrap();
        assert_eq!(
            delta.len(),
            1,
            "the origin still signs and exports it - the gate is the receiver's"
        );

        let store_b = InMemoryStore::new();
        let dir = keys(&[&a, &b]);
        let mut vv_b = VersionVector::default();
        let r = b.apply(&store_b, "ws", delta, &dir, &mut vv_b).unwrap();
        assert_eq!(r.accepted, 0, "a malformed event must not reach the log");
        assert!(
            matches!(r.rejected[0].reason, RejectReason::Malformed(_)),
            "expected Malformed, got: {:?}",
            r.rejected[0].reason
        );
        assert!(
            store_b.all_observations(Some("ws")).unwrap().is_empty(),
            "the permanent log stays clean"
        );
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
        store
            .add_observation(Observation::new("current fact".into(), prov("ws", 5)))
            .unwrap();

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
            (delta_b.iter().rev().cloned().collect(), delta_a.iter().rev().cloned().collect()),
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
