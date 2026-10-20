//! `supragnosis embed reset` (docs/semantic-retrieval.md Section 13): the product's binary against a
//! daemon in a home of its own, through the scenario harness (docs/e2e.md Section 3).

#![cfg(unix)]

#[path = "scenario/harness.rs"]
mod harness;

use harness::{wait_for, Daemon, Home, Mcp};
use serde_json::{json, Value};
use std::time::Duration;

/// SR9, with SR6: against a daemon that holds the store the reset changes nothing and names the way
/// to stop it; with the daemon stopped it removes every vector and the model the store recorded,
/// and says which model made them; a second reset finds nothing; and the next start records its
/// model and its backfill embeds every observation and entity again.
#[test]
fn embed_reset_refuses_a_held_store_and_reembeds_on_the_next_start() {
    let home = Home::new("embed-reset");
    let mut d = Daemon::start(&home);
    let mut app = Mcp::connect(&d, "claude-code");
    app.tool(
        "observe",
        json!({
            "workspace": "atlas-web",
            "content": "The checkout page renders on the server now.",
            "entities": [
                { "name": "Checkout page", "type": "Component" },
                { "name": "Server rendering", "type": "Decision" }
            ],
            "relations": [{ "from": "Checkout page", "type": "uses", "to": "Server rendering" }]
        }),
    )
    .expect("observe");
    drop(app);

    let held = d.cli(&["embed", "reset"]);
    let said = String::from_utf8_lossy(&held.stderr);
    assert!(!held.status.success(), "a held store is refused");
    assert!(
        said.contains("open in another process") && said.contains("supragnosis stop"),
        "the refusal names the way on: {said}"
    );

    d.stop();
    let reset = d.cli(&["embed", "reset"]);
    let out = String::from_utf8_lossy(&reset.stdout);
    assert!(
        reset.status.success(),
        "the reset runs: {}",
        String::from_utf8_lossy(&reset.stderr)
    );
    assert!(
        out.contains(
            "Removed 3 vectors (1 for observations, 2 for entities) made by 'hashing-256'"
        ),
        "it says what it removed and which model made it: {out}"
    );
    assert!(
        out.contains("The next start records 'hashing-256'"),
        "it names the model the daemon will run: {out}"
    );
    let again = d.cli(&["embed", "reset"]);
    assert!(
        String::from_utf8_lossy(&again.stdout).contains("nothing to reset"),
        "a second reset finds nothing: {}",
        String::from_utf8_lossy(&again.stdout)
    );

    let d = Daemon::restart(&home, &d);
    let backfill: Value = wait_for("the backfill to finish", Duration::from_secs(30), || {
        let o = d.cli(&["status", "--json"]);
        let v: Value = serde_json::from_slice(&o.stdout).map_err(|e| format!("not JSON: {e}"))?;
        let b = v["semantic"]["backfill"].clone();
        if b["state"] == "done" {
            Ok(b)
        } else {
            Err(format!("{b}"))
        }
    });
    assert_eq!(backfill["found"], 3, "every row was found without a vector: {backfill}");
    assert_eq!(backfill["embedded"], 3, "and embedded again: {backfill}");
}

/// SR10: the rebuild stops only a daemon this CLI manages for this home. Against a `serve` it did not
/// start - the scenario's own daemon - it refuses and changes nothing, whatever else this machine
/// runs: a login job of the person's is not this home's to stop, and is refused the same way.
#[test]
fn embed_rebuild_refuses_a_holder_it_does_not_manage_and_changes_nothing() {
    let home = Home::new("embed-rebuild");
    let mut d = Daemon::start(&home);
    let mut app = Mcp::connect(&d, "claude-code");
    app.tool(
        "observe",
        json!({
            "workspace": "atlas-web",
            "content": "The checkout page renders on the server now.",
            "entities": [{ "name": "Checkout page", "type": "Component" }]
        }),
    )
    .expect("observe");
    drop(app);

    let refused = d.cli(&["embed", "rebuild"]);
    let said = String::from_utf8_lossy(&refused.stderr);
    assert!(!refused.status.success(), "a holder this CLI does not manage is refused");
    assert!(said.contains("nothing was changed"), "the refusal says nothing changed: {said}");
    assert!(d.cli(&["status", "--json"]).status.success(), "the daemon still answers");

    d.stop();
    let reset = d.cli(&["embed", "reset"]);
    let out = String::from_utf8_lossy(&reset.stdout);
    assert!(
        out.contains(
            "Removed 2 vectors (1 for observations, 1 for entities) made by 'hashing-256'"
        ),
        "the store kept every vector through the refusal: {out}"
    );
}
