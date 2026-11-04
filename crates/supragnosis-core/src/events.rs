//! The observability event port and the transaction-time clock port (Principle 20).

use serde::Serialize;

use crate::{now_millis, Timestamp};

#[cfg(doc)]
use crate::ResolutionPolicy;

/// UI activity event (observability). Emitted by MCP tool calls **per intent**, and consumed by the viewer
/// for real-time log / node highlighting. The core defines only the data type + port (Principle 20) -
/// the actual transport (SSE/broadcast) is implemented by an adapter as [`EventSink`]. The node id lists
/// (entities/reached/id) are matched against graph nodes by the viewer for pulse/focus.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// Knowledge load: observation + linked entities/relations.
    Observe {
        observation: String,
        entities: Vec<String>,
        relations: usize,
        workspace: String,
    },
    /// Search: query + hits. `nodes` are hit ids (the viewer matches them against graph nodes to highlight -
    /// observation ids are not nodes, so they do not match and are ignored).
    Search {
        query: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
        hits: usize,
        nodes: Vec<String>,
        mode: String,
    },
    /// Entity lookup.
    GetEntity {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        found: bool,
    },
    /// Graph traversal: start node + reached nodes.
    Traverse { start: String, reached: Vec<String> },
    /// Federation sync activity (M4): a peer hit this node's sync API (hub side), or this node ran
    /// a round against a server (client side) - streamed to the viewer so remote hits are visible
    /// live in the activity feed.
    Sync {
        /// Hub side: "advertise" | "pull-served" | "push-received". Client side: "pull" | "push".
        direction: String,
        /// Counterparty: the peer node_id (hub side) or the server URL (client side).
        peer: String,
        workspace: String,
        /// Events served/accepted in this hit.
        count: usize,
    },
}

/// Event envelope: event + **session id** (the grouping key for a conversation footprint). To view together which
/// knowledge was used within the same session. `event` is flattened and serialized as `{session, kind, ...}` -
/// the viewer reads `ev.session` and `ev.kind` flat.
#[derive(Debug, Clone, Serialize)]
pub struct EventEnvelope {
    /// Session id (a conversation, roughly one MCP server run unit, or injected by the client). The footprint grouping key.
    pub session: String,
    /// The app of the local MCP session the activity came from, as it named itself in `initialize`
    /// (design-system.md Section 29.8).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(flatten)]
    pub event: Event,
}

/// UI event sink port (Principle 20). Held by the engine and emitted by tool calls. Without one it is a no-op -
/// observability is optional, and core correctness does not depend on this port (the spirit of Principle 19).
pub trait EventSink: Send + Sync {
    fn emit(&self, env: &EventEnvelope);
}

/// Transaction-time source port (Principle 20). The ingest path stamps `observed_at` from this, and
/// for a local (unstamped) attestation that timestamp becomes the ordering key `ordering_hlc` falls
/// back to - so the clock is not decoration, it decides which of two competing assertions the
/// resolution policy calls the more recent one.
///
/// It is a port because a test cannot otherwise say what time it is. Ingesting twice inside one
/// millisecond makes two observations tie on their ordering HLC and fall through to the id
/// tiebreak, which is a different branch of [`ResolutionPolicy`] than the one a recency test means
/// to exercise. The alternative is to sleep, and a test that sleeps is a test that asserts the
/// machine was fast enough - the wall clock the convergence guards are supposed to be free of.
pub trait Clock: Send + Sync {
    fn now_millis(&self) -> Timestamp;
}

/// The node wall clock - the default [`Clock`], and what every non-test caller wants.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_millis(&self) -> Timestamp {
        now_millis()
    }
}
