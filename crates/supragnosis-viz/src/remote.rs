//! The hub's read tier (docs/remote-viewer.md Section 3): the viewer served to the principals a hub
//! admits, read-only and within their grants.
//!
//! The local viewer answers this machine's owner over a 0600 unix socket, so it resolves an omitted
//! workspace to the node default, `*` to the whole store, and serves the console's writes. None of
//! that holds for a principal. This module is the other router: every path has a declared policy
//! ([`POLICY`]), every workspace is resolved against the reader's grants, and nothing on it changes
//! state. It takes the reader and the servable check as plain values (Principle 20) - which
//! credential admitted the reader, and which nodes consented to what, are the wiring layer's.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Map, Value};
use supragnosis_engine::Engine;

/// Whom the read tier answers: a principal's name and the workspaces it may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reader {
    pub name: String,
    pub readable: BTreeSet<String>,
}

/// Whether a workspace may be served remotely at all (remote-server.md R5): `Err` names the nodes
/// that have not consented. The same check the agent surface uses.
pub type Servable = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// What a path may do on the read tier (Section 3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    /// The page, its assets and what describes the surface - no workspace involved.
    Served,
    /// A read, resolved against the reader's grants.
    Read,
    /// Not on this surface, and why.
    Refused(&'static str),
}

const NODE_OP: &str = "a node's own operations and state - its peers, its sharing boundary, its \
     store's ledger - are not a principal's (docs/remote-viewer.md Section 3.2)";
const VERDICT: &str = "verdicts are cast from a node's own console, and the read tier changes \
     nothing (docs/remote-viewer.md V2)";
const WRITE: &str = "the read tier changes nothing - a principal with a write grant reaches \
     `observe` and `propose` through MCP at /mcp (docs/remote-viewer.md Section 2)";

/// Every path the viewer serves, and what it may do here. A path missing from this list is not
/// served (V2), and a test holds the list to every path the local router answers, so an endpoint
/// added to the viewer later stays closed until someone decides what it may do.
pub const POLICY: &[(&str, Policy)] = &[
    ("/", Policy::Served),
    ("/viewer.css", Policy::Served),
    ("/viewer.js", Policy::Served),
    ("/api/about", Policy::Served),
    ("/api/surface", Policy::Served),
    ("/api/workspaces", Policy::Read),
    ("/api/graph", Policy::Read),
    ("/api/hypergraph", Policy::Read),
    ("/api/types", Policy::Read),
    ("/api/curation", Policy::Read),
    ("/api/proposals", Policy::Read),
    ("/api/observations", Policy::Read),
    ("/api/proposal", Policy::Read),
    ("/api/explain", Policy::Read),
    ("/api/events", Policy::Read),
    ("/api/federation", Policy::Refused(NODE_OP)),
    ("/api/peer/share", Policy::Refused(NODE_OP)),
    ("/api/health", Policy::Refused(NODE_OP)),
    ("/api/review", Policy::Refused(VERDICT)),
    ("/api/resolve", Policy::Refused(VERDICT)),
    ("/api/reify", Policy::Refused(WRITE)),
    ("/api/propose_merge", Policy::Refused(WRITE)),
    ("/api/propose_split", Policy::Refused(WRITE)),
];

pub fn policy(path: &str) -> Option<Policy> {
    POLICY.iter().find(|(p, _)| *p == path).map(|(_, p)| *p)
}

/// One answer of the read tier. The wiring layer adds the headers every viewer answer carries
/// ([`crate::CSP`] among them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Answer {
    fn json(status: u16, body: String) -> Answer {
        Answer { status, content_type: "application/json", body }
    }

    fn error(status: u16, msg: &str) -> Answer {
        Answer::json(status, crate::err_body(msg))
    }

    /// An entity or observation id that is unknown - or outside the grants, which must read the
    /// same: ids are computable from a workspace and a name or a content (Section 3.4).
    fn unknown_id() -> Answer {
        Answer::error(
            404,
            "no entity with that id in the workspaces you may read - unknown, not a negation \
             (open-world)",
        )
    }

    fn from_local(r: crate::Response) -> Answer {
        let status = r.status.get(..3).and_then(|s| s.parse().ok()).unwrap_or(500);
        Answer { status, content_type: r.content_type, body: r.body }
    }
}

/// Answers one request on the read tier. Only GET; a path without a policy is not served; a refused
/// path says why and where the act can be done instead.
pub fn route(
    engine: &Engine,
    reader: &Reader,
    servable: &Servable,
    method: &str,
    path: &str,
    query: &str,
) -> Answer {
    if method != "GET" {
        return Answer::error(405, "the read tier answers GET only (docs/remote-viewer.md V2)");
    }
    match policy(path) {
        None => Answer::error(
            404,
            "not served on the read tier - the paths it serves are in docs/remote-viewer.md \
             Section 3.2",
        ),
        Some(Policy::Refused(why)) => Answer::error(403, why),
        Some(Policy::Served) if path == "/api/surface" => {
            Answer::json(200, surface(reader, servable).to_string())
        }
        // The page and its assets are the local router's, unchanged: the page decides read-only from
        // /api/surface, so one page serves both surfaces.
        Some(Policy::Served) => Answer::from_local(crate::route(engine, "GET", path, "")),
        Some(Policy::Read) => read(engine, reader, servable, path, query),
    }
}

/// `/api/surface` on the read tier (Section 3.5): who the reader is, that the surface is read-only,
/// the workspace an omitted one resolves to, and each grant with whether the hub may serve it.
pub fn surface(reader: &Reader, servable: &Servable) -> Value {
    let workspaces: Vec<Value> = reader
        .readable
        .iter()
        .map(|w| match servable(w) {
            Ok(()) => serde_json::json!({ "name": w, "servable": true }),
            Err(why) => serde_json::json!({ "name": w, "servable": false, "reason": why }),
        })
        .collect();
    serde_json::json!({
        "surface": "remote",
        "principal": reader.name,
        "read_only": true,
        "default_workspace": reader.readable.iter().next(),
        "workspaces": workspaces,
    })
}

/// One `/api/events` frame as the read tier relays it (Section 3.6): an `observe` in a workspace the
/// reader may read and the hub may serve, without the session it came from. Every other event -
/// searches, lookups, traversals, sync rounds - is somebody's activity, not knowledge, and is
/// dropped (V7).
pub fn filter_event(json: &str, reader: &Reader, servable: &Servable) -> Option<String> {
    let mut v: Value = serde_json::from_str(json).ok()?;
    if v.get("kind").and_then(Value::as_str) != Some("observe") {
        return None;
    }
    let ws = v.get("workspace").and_then(Value::as_str)?.to_string();
    if !reader.readable.contains(&ws) || servable(&ws).is_err() {
        return None;
    }
    v.as_object_mut()?.remove("session");
    Some(v.to_string())
}

fn param(query: &str, key: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{key}=")))
        .map(crate::percent_decode)
}

/// The workspaces one read covers (Section 3.3).
enum Scope {
    One(String),
    Union(Vec<String>),
}

/// Resolves a requested workspace against the grants: omitted is the first readable grant, a name
/// must be granted and servable, and `*` is every grant - refused as a whole if any one of them may
/// not be served, rather than shown without it (V4).
fn resolve(reader: &Reader, servable: &Servable, raw: Option<&str>) -> Result<Scope, Answer> {
    let granted = || {
        if reader.readable.is_empty() {
            "none".to_string()
        } else {
            reader.readable.iter().cloned().collect::<Vec<_>>().join(", ")
        }
    };
    match raw.map(str::trim) {
        None => match reader.readable.iter().next() {
            Some(ws) => {
                servable(ws).map_err(|why| Answer::error(403, &why))?;
                Ok(Scope::One(ws.clone()))
            }
            None => Err(Answer::error(403, &format!("{} may read no workspace", reader.name))),
        },
        Some("" | "*" | "all") => {
            if reader.readable.is_empty() {
                return Err(Answer::error(403, &format!("{} may read no workspace", reader.name)));
            }
            for ws in &reader.readable {
                servable(ws).map_err(|why| Answer::error(403, &why))?;
            }
            Ok(Scope::Union(reader.readable.iter().cloned().collect()))
        }
        Some(ws) if reader.readable.contains(ws) => {
            servable(ws).map_err(|why| Answer::error(403, &why))?;
            Ok(Scope::One(ws.to_string()))
        }
        Some(ws) => Err(Answer::error(
            403,
            &format!(
                "workspace {ws:?} is not granted to {} - you may read: {}",
                reader.name,
                granted()
            ),
        )),
    }
}

/// The workspace an entity id resolves to, if the reader may see it there. Outside the grants it is
/// the same answer as an unknown id (Section 3.4); a granted workspace the hub may not serve says
/// why, because the reader holds that grant.
fn visible_entity(
    engine: &Engine,
    reader: &Reader,
    servable: &Servable,
    id: &str,
) -> Result<String, Answer> {
    match engine.get_entity(id) {
        Ok(Some(view)) => {
            let ws =
                view.entity.provenance.first().map(|p| p.workspace.clone()).unwrap_or_default();
            if !reader.readable.contains(&ws) {
                return Err(Answer::unknown_id());
            }
            servable(&ws).map_err(|why| Answer::error(403, &why))?;
            Ok(ws)
        }
        Ok(None) => Err(Answer::unknown_id()),
        Err(e) => Err(failure(&e)),
    }
}

fn failure(e: &impl std::fmt::Display) -> Answer {
    Answer::json(
        500,
        serde_json::json!({
            "error": e.to_string(),
            "note": "storage backend failure - NOT an empty answer (Principle 5)"
        })
        .to_string(),
    )
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, Answer> {
    serde_json::to_value(v).map_err(|e| Answer::error(500, &format!("serialize error: {e}")))
}

fn read(engine: &Engine, reader: &Reader, servable: &Servable, path: &str, query: &str) -> Answer {
    match read_value(engine, reader, servable, path, query) {
        Ok(v) => Answer::json(200, v.to_string()),
        Err(a) => a,
    }
}

fn read_value(
    engine: &Engine,
    reader: &Reader,
    servable: &Servable,
    path: &str,
    query: &str,
) -> Result<Value, Answer> {
    let limit = param(query, "limit").and_then(|s| s.parse::<usize>().ok());
    match path {
        // Enumeration is the grants that hold knowledge, the unservable ones included: a grant the
        // hub may not serve is shown with its reason on /api/surface, not left out (P5).
        "/api/workspaces" => {
            let held = engine.workspaces().map_err(|e| failure(&e))?;
            to_value(held.into_iter().filter(|w| reader.readable.contains(w)).collect::<Vec<_>>())
        }
        "/api/explain" => {
            let Some(id) = param(query, "entity").filter(|s| !s.is_empty()) else {
                return Err(Answer::error(400, "explain needs ?entity=<id>"));
            };
            visible_entity(engine, reader, servable, &id)?;
            match engine.explain_entity(&id) {
                Ok(Some(ex)) => to_value(ex),
                Ok(None) => Err(Answer::unknown_id()),
                Err(e) => Err(failure(&e)),
            }
        }
        // One entity's evidence: its own workspace decides, whatever the request named.
        "/api/observations" if param(query, "entity").is_some_and(|s| !s.is_empty()) => {
            let id = param(query, "entity").unwrap_or_default();
            let ws = visible_entity(engine, reader, servable, &id)?;
            to_value(engine.observation_log(Some(&ws), Some(&id), limit).map_err(|e| failure(&e))?)
        }
        // A proposal lives in one workspace, so `*` is not a scope it can be read in.
        "/api/proposal" => {
            let Some(id) = param(query, "id").filter(|s| !s.is_empty()) else {
                return Err(Answer::error(400, "proposal needs ?id=<proposal id>"));
            };
            let ws = match resolve(reader, servable, param(query, "workspace").as_deref())? {
                Scope::One(ws) => ws,
                Scope::Union(_) => {
                    return Err(Answer::error(
                        400,
                        "a proposal lives in one workspace - name it with ?workspace=",
                    ))
                }
            };
            match engine.get_proposal(Some(&ws), &id) {
                Ok(Some(view)) => to_value(view),
                Ok(None) => Err(Answer::error(404, "no proposal with that id in this workspace")),
                Err(e) => Err(Answer::error(500, &format!("store error: {e}"))),
            }
        }
        "/api/graph" | "/api/hypergraph" | "/api/types" | "/api/curation" | "/api/proposals"
        | "/api/observations" => {
            match resolve(reader, servable, param(query, "workspace").as_deref())? {
                Scope::One(ws) => project(engine, path, &ws, limit),
                Scope::Union(all) => {
                    let mut merged: Option<Value> = None;
                    for ws in &all {
                        let v = project(engine, path, ws, limit)?;
                        merged = Some(match merged {
                            None => v,
                            Some(m) => merge(m, v),
                        });
                    }
                    let mut out = merged.unwrap_or(Value::Null);
                    if let Value::Object(o) = &mut out {
                        // The local `*` answer has no `workspace`: a union is not one of them.
                        o.remove("workspace");
                    }
                    if path == "/api/observations" {
                        out = newest_first(out, limit);
                    }
                    Ok(out)
                }
            }
        }
        _ => Err(Answer::error(404, "not a read on this surface")),
    }
}

/// One workspace's projection, exactly as the local viewer answers it for that workspace.
fn project(engine: &Engine, path: &str, ws: &str, limit: Option<usize>) -> Result<Value, Answer> {
    let ws = Some(ws);
    match path {
        "/api/graph" => to_value(engine.graph(ws).map_err(|e| failure(&e))?),
        "/api/hypergraph" => to_value(engine.hypergraph(ws).map_err(|e| failure(&e))?),
        "/api/types" => to_value(engine.types(ws).map_err(|e| failure(&e))?),
        "/api/curation" => to_value(engine.curation(ws).map_err(|e| failure(&e))?),
        "/api/proposals" => to_value(engine.list_proposals(ws).map_err(|e| failure(&e))?),
        "/api/observations" => {
            to_value(engine.observation_log(ws, None, limit).map_err(|e| failure(&e))?)
        }
        _ => Err(Answer::error(404, "not a projection")),
    }
}

/// Merges two workspaces' answers into one (Section 3.3): lists are concatenated, counts summed - a
/// `max_` statistic takes the larger - flags or-ed, and objects merged field by field. Nothing is
/// computed across the two, so a union shows exactly what each workspace shows on its own.
fn merge(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Array(mut x), Value::Array(y)) => {
            x.extend(y);
            Value::Array(x)
        }
        (Value::Object(x), Value::Object(y)) => Value::Object(merge_objects(x, y)),
        (_, b) => b,
    }
}

fn merge_objects(mut x: Map<String, Value>, y: Map<String, Value>) -> Map<String, Value> {
    for (k, v) in y {
        let merged = match x.remove(&k) {
            None => v,
            Some(old) => merge_field(&k, old, v),
        };
        x.insert(k, merged);
    }
    x
}

fn merge_field(key: &str, a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            let n = if key.starts_with("max_") { x.max(y) } else { x + y };
            if n.fract() == 0.0 && n >= 0.0 {
                Value::from(n as u64)
            } else {
                Value::from(n)
            }
        }
        (Value::Bool(x), Value::Bool(y)) => Value::Bool(x || y),
        (Value::String(x), Value::String(y)) if x == y => Value::String(x),
        // Two different strings for one key (a workspace name) describe neither workspace alone.
        (Value::String(_), Value::String(_)) => Value::Null,
        (a, b) => merge(a, b),
    }
}

/// Observations are newest first; a union of two such lists is re-sorted, then cut to the limit.
fn newest_first(v: Value, limit: Option<usize>) -> Value {
    let Value::Array(mut rows) = v else {
        return v;
    };
    let key = |o: &Value| {
        (
            o["hlc"]["wall"].as_u64().unwrap_or(0),
            o["hlc"]["counter"].as_u64().unwrap_or(0),
            o["id"].as_str().unwrap_or("").to_string(),
        )
    };
    rows.sort_by_key(|o| std::cmp::Reverse(key(o)));
    if let Some(n) = limit {
        rows.truncate(n);
    }
    Value::Array(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_union_sums_counts_and_keeps_the_larger_max() {
        let a = serde_json::json!({"workspace": "a", "nodes": [1], "stats": {"node_count": 1, "max_size": 4, "type_counts": {"T": 1}}, "merge_band": {"available": false}});
        let b = serde_json::json!({"workspace": "b", "nodes": [2, 3], "stats": {"node_count": 2, "max_size": 3, "type_counts": {"T": 2, "U": 1}}, "merge_band": {"available": true}});
        let m = merge(a, b);
        assert_eq!(m["nodes"], serde_json::json!([1, 2, 3]));
        assert_eq!(m["stats"]["node_count"], 3);
        assert_eq!(m["stats"]["max_size"], 4);
        assert_eq!(m["stats"]["type_counts"], serde_json::json!({"T": 3, "U": 1}));
        assert_eq!(m["merge_band"]["available"], true);
        assert!(m["workspace"].is_null(), "two names describe neither workspace");
    }
}
