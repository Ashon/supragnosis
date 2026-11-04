//! Sync transport (M4 Phase 3, docs/federation.md Section 6a): the axum sync API
//! (`/sync/advertise|pull|push`) with in-process rustls TLS and bearer/allowlist wire auth, plus the
//! reqwest client. This is the ONLY surface permitted to bind non-loopback, and only with TLS enabled
//! and at least one admitted node or open invitation (F10, pairing.md PA1) - the MCP/viz loopback
//! guard is untouched. Wire
//! auth here is transport authentication (who is on the wire); content trust stays with the apply
//! pipeline's signature verification (F6) and the read side (F13).

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use supragnosis_core::{AssertionStore, AttestationEvent, SearchHit, VersionVector};

use crate::{export_delta, version_vector, ApplyReport, SyncError, SyncNode};

mod client;
mod handlers;
mod server;
mod tls;
mod wire;

pub use client::*;
pub use handlers::*;
pub use server::*;
pub use tls::*;
pub use wire::*;

/// The bounds of one sync request (sync-correctness.md Section 11). Every event fits inside them,
/// because `observe` caps content at 1 MiB.
///
/// The body limit the sync routes declare, rather than inheriting axum's unnamed 2 MB default.
pub const BODY_LIMIT: usize = 8 << 20;
/// A push batch: at most this many encoded bytes, or [`BATCH_EVENTS`] events, whichever comes first.
pub const BATCH_BYTES: usize = 1 << 20;
pub const BATCH_EVENTS: usize = 500;
/// A pull page: at most the events the client asks for (at most [`PAGE_EVENTS`]), and at most
/// [`PAGE_BYTES`] encoded - always at least one event, so a page always makes progress.
pub const PAGE_EVENTS: usize = 500;
pub const PAGE_BYTES: usize = 4 << 20;
/// The sync protocol's version (sync-correctness.md Section 12). Raised by a change an older peer
/// would mishandle - a new enum value on the wire, a new signed field, a changed meaning - and not
/// by an optional field, an endpoint or a header, which older peers ignore. A peer that sends none
/// speaks protocol 0: everything before this number existed.
pub const PROTOCOL: u32 = 1;
/// The headers every request and every response carry: the protocol, and the binary's release.
pub const PROTOCOL_HEADER: &str = "supragnosis-sync";
pub const RELEASE_HEADER: &str = "supragnosis-release";

/// The header a 0.10 node sent its consent to serve a workspace in, which a 0.10 hub read to serve
/// it to its principals (docs/remote-server.md Section 4.5). Principals were removed in 0.11.0, so a
/// 0.11 node says "no" in it on each advertise, pull and push, and a 0.10 hub withdraws its consent
/// for the workspaces the two still sync (remote-server.md Section 13); revisit when no 0.10 hub
/// remains.
pub const SERVE_HEADER: &str = "supragnosis-serve";

/// How long one sync request may take, on either side.
pub const CALL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// What a refused pin says, so a joining machine can tell "not the machine that made the code" from
/// a host it could not reach (docs/pairing.md Section 4.3).
pub const NOT_THE_CODES_HUB: &str = "the certificate is not the one the join code names";

/// Transport failure. Distinct from per-event rejections (those ride the apply report, F6) and from
/// store failures (P5: surfaced, never silent).
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error(
        "refusing to bind {addr}: {reason} (F10: non-loopback needs TLS and an admitted node or an open invitation)"
    )]
    Bind { addr: SocketAddr, reason: String },
    #[error("tls setup failed: {0}")]
    Tls(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("server returned {status}: {body}")]
    Remote { status: u16, body: String },
    #[error(transparent)]
    Sync(#[from] SyncError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl TransportError {
    /// Whether the host was not reached at all - a partition, not an answer.
    pub fn unreachable(&self) -> bool {
        matches!(self, TransportError::Http(e) if e.is_connect() || e.is_timeout())
    }

    /// What a failed call says to a person: the error and every cause under it, and, when the host
    /// was not reached, that this node goes on offline and syncs once it is reached again - a
    /// partition is a state a node works through, not a fault (docs/pairing.md, decision D16). On a
    /// Mac, "no route" is also how macOS refuses a process without Local Network access.
    pub fn said(&self) -> String {
        let mut out = self.to_string();
        let mut cause = std::error::Error::source(self);
        while let Some(c) = cause {
            let text = c.to_string();
            if !out.contains(&text) {
                out.push_str(": ");
                out.push_str(&text);
            }
            cause = c.source();
        }
        if self.unreachable() {
            out.push_str(
                ". This node keeps working offline, and its next round syncs once the hub can be \
                 reached",
            );
            let no_route = out.contains("os error 65") || out.contains("No route to host");
            if cfg!(target_os = "macos") && no_route {
                out.push_str(
                    ". On a Mac, macOS answers this way for a process without Local Network \
                     access (System Settings > Privacy & Security > Local Network)",
                );
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
