//! The remote surface - what a principal the hub admits may do through MCP (docs/remote-server.md
//! Section 4).
//!
//! The local daemon's surface is unchanged and has no principal: it is this machine's. The hub's
//! listener serves the same tools to principals, and every call passes through [`admit`] first: a
//! pure function from the principal, the tool and its arguments to either the arguments the call
//! may run with or the reason it may not. Tools that resolve a workspace from an id - an entity, an
//! observation - check it themselves through [`current`], the principal of the call in progress.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Map, Value};

/// A person or an agent the hub admits, with per-workspace grants. `write` implies `read`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub name: String,
    pub read: BTreeSet<String>,
    pub write: BTreeSet<String>,
}

impl Principal {
    pub fn can_read(&self, ws: &str) -> bool {
        self.read.contains(ws) || self.write.contains(ws)
    }

    pub fn can_write(&self, ws: &str) -> bool {
        self.write.contains(ws)
    }

    /// Every workspace this principal may read, in name order.
    pub fn readable(&self) -> Vec<String> {
        self.read.union(&self.write).cloned().collect()
    }

    fn granted_list(&self) -> String {
        let r = self.readable();
        if r.is_empty() {
            "none".into()
        } else {
            r.join(", ")
        }
    }
}

/// What a tool may do on the remote surface (Section 4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Read,
    Write,
    Refused(&'static str),
}

/// Every tool's remote policy. A tool missing from this list is refused (R3), and a test holds the
/// list equal to the tools the server declares, so a new tool cannot arrive open.
pub const POLICY: &[(&str, Policy)] = &[
    ("search_knowledge", Policy::Read),
    ("get_entity", Policy::Read),
    ("traverse", Policy::Read),
    ("workspace_map", Policy::Read),
    ("list_proposals", Policy::Read),
    ("get_proposal", Policy::Read),
    ("observe", Policy::Write),
    ("propose", Policy::Write),
    (
        "review",
        Policy::Refused(
            "a verdict needs the principal's own signing key, which remote principals do not have \
             yet - cast it from a node, or ask the hub's operator (docs/remote-server.md R7)",
        ),
    ),
    (
        "define_type",
        Policy::Refused(
            "a T-Box change waits for principal-signed acts and the tbox_change gate \
             (docs/remote-server.md R7)",
        ),
    ),
    ("sync_status", Policy::Refused("a node operation, not a principal's")),
    ("sync_pull", Policy::Refused("a node operation, not a principal's")),
    ("sync_push", Policy::Refused("a node operation, not a principal's")),
];

pub fn policy(tool: &str) -> Option<Policy> {
    POLICY.iter().find(|(n, _)| *n == tool).map(|(_, p)| *p)
}

/// Whether a workspace argument asks for every workspace.
pub fn is_all(ws: &str) -> bool {
    matches!(ws.trim(), "" | "*" | "all")
}

fn workspace_arg(args: &Map<String, Value>) -> Option<String> {
    args.get("workspace").and_then(Value::as_str).map(|s| s.trim().to_string())
}

/// Admits one remote call, rewriting its arguments to what this principal may do, or says why it
/// may not. Pure: the same principal, tool and arguments always give the same answer.
///
/// - Reads name a granted workspace; an omitted one becomes the principal's first read grant.
///   `search_knowledge` alone may ask for all of them (`*`), meaning the union of its grants.
/// - Writes name a workspace the principal may write; an omitted one becomes its first write grant.
///   `on_behalf_of` is set to the principal, whatever the client sent (R4).
pub fn admit(p: &Principal, tool: &str, args: &mut Map<String, Value>) -> Result<(), String> {
    match policy(tool) {
        None => Err(format!(
            "{tool} has no remote policy, so it is closed (docs/remote-server.md R3)"
        )),
        Some(Policy::Refused(why)) => {
            Err(format!("{tool} is not available on the remote surface: {why}"))
        }
        Some(Policy::Read) => admit_read(p, tool, args),
        Some(Policy::Write) => admit_write(p, tool, args),
    }
}

fn admit_read(p: &Principal, tool: &str, args: &mut Map<String, Value>) -> Result<(), String> {
    if tool == "search_knowledge" {
        if let Some(scope) = args.get("scope").and_then(Value::as_str) {
            if scope == "remote" || scope == "both" {
                return Err(format!(
                    "scope {scope:?} is not available remotely - this server does not fan a \
                     principal's query out to its peers"
                ));
            }
        }
    }
    // Id-based reads resolve their workspace from the id and check it themselves.
    if tool == "get_entity" || tool == "traverse" {
        return Ok(());
    }
    match workspace_arg(args) {
        Some(ws) if is_all(&ws) && tool == "search_knowledge" => {
            if p.readable().is_empty() {
                return Err("this principal may read no workspace".into());
            }
            args.insert("workspace".into(), Value::String("*".into()));
            Ok(())
        }
        Some(ws) if is_all(&ws) => Err(format!(
            "{tool} needs one workspace on the remote surface - you may read: {}",
            p.granted_list()
        )),
        Some(ws) if p.can_read(&ws) => Ok(()),
        Some(ws) => Err(format!(
            "workspace {ws:?} is not granted to {} - you may read: {}",
            p.name,
            p.granted_list()
        )),
        None => match p.readable().first() {
            Some(ws) => {
                args.insert("workspace".into(), Value::String(ws.clone()));
                Ok(())
            }
            None => Err("this principal may read no workspace".into()),
        },
    }
}

fn admit_write(p: &Principal, tool: &str, args: &mut Map<String, Value>) -> Result<(), String> {
    let ws = match workspace_arg(args) {
        Some(ws) if is_all(&ws) => {
            return Err(format!("{tool} writes to one workspace - name it"));
        }
        Some(ws) => ws,
        None => p
            .write
            .iter()
            .next()
            .cloned()
            .ok_or_else(|| format!("{} may not write to any workspace", p.name))?,
    };
    if !p.can_write(&ws) {
        return Err(format!(
            "workspace {ws:?} is not writable by {} - write grants: {}",
            p.name,
            if p.write.is_empty() {
                "none".into()
            } else {
                p.write.iter().cloned().collect::<Vec<_>>().join(", ")
            }
        ));
    }
    args.insert("workspace".into(), Value::String(ws));
    // R4: the authenticated principal is the fact; a client's own claim is not.
    args.insert("on_behalf_of".into(), Value::String(p.name.clone()));
    Ok(())
}

/// Whether a workspace may be served remotely at all (R5): every node whose attestations it holds
/// is this one, or has consented. Supplied by the wiring, which knows the node's id and the consent
/// it has recorded; `Err` names what is missing.
pub type Servable = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// The remote surface's configuration, attached to a server that serves principals.
#[derive(Clone)]
pub struct Surface {
    pub servable: Servable,
}

tokio::task_local! {
    static PRINCIPAL: Principal;
}

/// The principal of the remote call in progress; `None` on the local surface.
pub fn current() -> Option<Principal> {
    PRINCIPAL.try_with(Clone::clone).ok()
}

/// Runs `f` as `p`'s call, so tools that resolve a workspace from an id can check it.
pub async fn as_principal<F: std::future::Future>(p: Principal, f: F) -> F::Output {
    PRINCIPAL.scope(p, f).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alice() -> Principal {
        Principal {
            name: "alice".into(),
            read: ["team".to_string(), "docs".to_string()].into(),
            write: ["team".to_string()].into(),
        }
    }

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    /// R3: reads name a granted workspace (an omitted one becomes the first grant), only search may
    /// ask for all of them, and nothing reads past the grants.
    #[test]
    fn a_principal_reads_only_its_grants() {
        let p = alice();
        let mut a = args(serde_json::json!({"query": "x"}));
        admit(&p, "search_knowledge", &mut a).unwrap();
        assert_eq!(a["workspace"], "docs", "omitted -> first read grant");

        let mut a = args(serde_json::json!({"query": "x", "workspace": "*"}));
        admit(&p, "search_knowledge", &mut a).unwrap();
        assert_eq!(a["workspace"], "*", "search may ask for the union of grants");

        let mut a = args(serde_json::json!({"workspace": "*"}));
        assert!(admit(&p, "workspace_map", &mut a)
            .unwrap_err()
            .contains("you may read: docs, team"));
        let mut a = args(serde_json::json!({"workspace": "secret"}));
        assert!(admit(&p, "list_proposals", &mut a).unwrap_err().contains("not granted"));
        let mut a = args(serde_json::json!({"query": "x", "scope": "both"}));
        assert!(admit(&p, "search_knowledge", &mut a).is_err(), "no fan-out to peers");
    }

    /// R3/R4: writes go only where granted, and the principal - not the client's claim - is who
    /// the observation is on behalf of.
    #[test]
    fn a_remote_write_is_the_principals_own() {
        let p = alice();
        let mut a = args(serde_json::json!({"content": "c", "on_behalf_of": "mallory"}));
        admit(&p, "observe", &mut a).unwrap();
        assert_eq!(a["workspace"], "team", "omitted -> first write grant");
        assert_eq!(a["on_behalf_of"], "alice", "the client's claim is replaced");

        let mut a = args(serde_json::json!({"content": "c", "workspace": "docs"}));
        assert!(admit(&p, "observe", &mut a).unwrap_err().contains("not writable"));
        let reader = Principal { write: BTreeSet::new(), ..alice() };
        let mut a = args(serde_json::json!({"content": "c"}));
        assert!(admit(&reader, "propose", &mut a).is_err());
    }

    /// R7: verdicts, T-Box changes and node operations are refused, and so is a tool with no policy.
    #[test]
    fn governance_and_node_operations_are_refused_remotely() {
        let p = alice();
        for tool in
            ["review", "define_type", "sync_push", "sync_pull", "sync_status", "no_such_tool"]
        {
            let mut a = Map::new();
            assert!(admit(&p, tool, &mut a).is_err(), "{tool}");
        }
    }
}
