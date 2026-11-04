//! The sync API's wire types: what each route takes and answers, and how events cross the wire.

use super::*;

/// The (origin, seq) of a wire event, read without decoding it. Empty for an unstamped one.
pub(super) fn stream_of(raw: &serde_json::Value) -> (String, u64) {
    let sync = &raw["attestation"]["sync"];
    (
        sync["origin_node"].as_str().unwrap_or_default().to_string(),
        sync["origin_seq"].as_u64().unwrap_or_default(),
    )
}

// --- wire types --------------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize)]
pub struct AdvertiseReq {
    pub workspace: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdvertiseResp {
    pub node_id: String,
    pub vv: VersionVector,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PullReq {
    pub workspace: String,
    pub since: VersionVector,
    /// The most events wanted in one page. An older hub ignores it and answers with everything,
    /// which a newer client reads as one page; an older client sends none and gets one page of
    /// [`PAGE_EVENTS`], continued by its next round (sync-correctness.md Section 11).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Events cross the wire as JSON values and are decoded one at a time on receipt
/// (sync-correctness.md Section 6): an event this release cannot decode is rejected alone, where a
/// typed list would fail the whole body - every other event with it - every round.
#[derive(Debug, Serialize, Deserialize)]
pub struct PullResp {
    pub events: Vec<serde_json::Value>,
    /// More events remain beyond this page. Absent from an older hub, which sends everything.
    #[serde(default)]
    pub more: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushReq {
    pub workspace: String,
    pub events: Vec<serde_json::Value>,
}

/// Whether a rejection a host reported holds its stream there ([`crate::RejectReason::holds`]). A reason
/// travels as its `Debug` form, so the ones that do not hold are told apart by name; an older host
/// holds them too, and then only answers `Held` for what is sent behind them.
pub(super) fn holds_at_the_host(reason: &str) -> bool {
    !["Malformed", "WorkspaceMismatch", "Unstamped"]
        .iter()
        .any(|r| reason.starts_with(r))
}

/// The wire form of outgoing events.
pub(super) fn to_wire(events: Vec<AttestationEvent>) -> Vec<serde_json::Value> {
    events
        .into_iter()
        .map(|e| serde_json::to_value(e).expect("an event serializes"))
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchReq {
    pub workspace: String,
    pub query: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResp {
    /// The retrieval surface the SERVER used (hybrid/keyword) - results are the server's node-local
    /// retrieval aid (P16 exemption): to become local graph material they must arrive via pull (F1).
    pub mode: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PingResp {
    /// The hub's node id.
    pub node_id: String,
    /// The hub's binary version.
    pub version: String,
    /// The hub's sync protocol (sync-correctness.md Section 12). 0 from a hub before it.
    #[serde(default)]
    pub protocol: u32,
    /// The workspaces the CALLER is authorized to sync (diagnostics for setup mistakes).
    pub shared_workspaces: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PushResp {
    pub accepted: usize,
    /// (origin_node, origin_seq, reason) - rejections are reported, never silently dropped (P5).
    pub rejected: Vec<(String, u64, String)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RejectReason;

    /// Section 16 (D13): a pushing client reads which rejections hold their stream at the host from
    /// the reasons the host reports, so it keeps sending a stream whose rejection was dropped and
    /// stops one that is held - the same split the host applies.
    #[test]
    fn a_client_reads_which_rejections_hold_as_its_host_applies_them() {
        for reason in [
            RejectReason::Unstamped,
            RejectReason::UnknownOrigin,
            RejectReason::BadSignature,
            RejectReason::WorkspaceMismatch,
            RejectReason::Malformed("confidence 5".into()),
            RejectReason::Undecodable("unknown variant".into()),
            RejectReason::Held(3),
            RejectReason::OriginNotAdmitted,
        ] {
            assert_eq!(holds_at_the_host(&format!("{reason:?}")), reason.holds(), "{reason:?}");
        }
    }
}
