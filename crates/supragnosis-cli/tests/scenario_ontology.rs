//! Ontology scenarios (docs/e2e.md Section 5.1): the product's binary, started as `supragnosis serve`
//! in a home of its own, used the way an AI app and a person at the console use it. Each test's
//! name begins with the id of the scenario it tells (J8).

#[path = "scenario/harness.rs"]
mod harness;

use harness::{http, Daemon, Home, Mcp};
use serde_json::{json, Value};

fn graph(d: &Daemon, ws: &str) -> Value {
    let (status, g) = d.viz(&format!("/api/graph?workspace={ws}"));
    assert_eq!(status, 200, "the graph answers: {g}");
    g
}

fn node<'a>(g: &'a Value, name: &str) -> Option<&'a Value> {
    g["nodes"].as_array()?.iter().find(|n| n["name"] == name)
}

/// A1. Record and recall: what an agent observes is found by search, read back with who said it,
/// connected to what it named, and returned word for word from the log.
#[test]
fn a1_record_and_recall() {
    let home = Home::new("a1");
    let d = Daemon::start(&home);
    let mut app = Mcp::connect(&d, "claude-code");
    let obs = app
        .tool(
            "observe",
            json!({
                "workspace": "atlas-web",
                "content": "We render the checkout page on the server now: first paint fell from 2.4s to 1.3s.",
                "entities": [
                    { "name": "Checkout page", "type": "Component", "description": "The page where a customer pays." },
                    { "name": "Server rendering", "type": "Decision" }
                ],
                "relations": [{ "from": "Checkout page", "type": "uses", "to": "Server rendering" }],
                "on_behalf_of": "sam",
                "source_ref": "git:4f2a9c1"
            }),
        )
        .expect("observe");
    let obs_id = obs["observation_id"].as_str().expect("an observation id").to_string();

    let found = app
        .tool("search_knowledge", json!({ "query": "checkout", "workspace": "atlas-web" }))
        .unwrap();
    let hits = found["hits"].as_array().expect("hits");
    assert!(
        hits.iter()
            .any(|h| h["snippet"].as_str().is_some_and(|s| s.contains("Checkout page"))),
        "{found}"
    );

    let g = graph(&d, "atlas-web");
    let checkout = node(&g, "Checkout page").expect("the entity is in the graph");
    let id = checkout["id"].as_str().unwrap();
    let ent = app.tool("get_entity", json!({ "id": id })).unwrap();
    assert_eq!(ent["canonical_name"], "Checkout page", "{ent}");
    assert_eq!(ent["relations"].as_array().map(Vec::len), Some(1), "{ent}");

    let reached = app.tool("traverse", json!({ "id": id })).unwrap();
    assert!(
        reached["hits"]
            .as_array()
            .is_some_and(|a| a.iter().any(|h| h["name"] == "Server rendering")),
        "{reached}"
    );

    let read = app
        .rpc(
            "resources/read",
            json!({ "uri": format!("supragnosis://observation/{obs_id}") }),
        )
        .unwrap();
    let text: Value = serde_json::from_str(read["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert!(
        text["content"].as_str().unwrap().starts_with("We render the checkout page"),
        "{text}"
    );
    assert_eq!(text["provenance"][0]["on_behalf_of"], "sam", "who it was said for: {text}");
    assert_eq!(text["provenance"][0]["source_ref"], "git:4f2a9c1", "{text}");
}

/// A2. Contested, then settled by a person: two readings of one entity's type stay side by side
/// until the console confirms one, and the other is kept as a competitor (P6).
#[test]
fn a2_contested_then_settled_by_a_person() {
    let home = Home::new("a2");
    let d = Daemon::start(&home);
    let mut app = Mcp::connect(&d, "claude-code");
    let mut obs = Vec::new();
    for kind in ["Database", "Technology"] {
        let r = app
            .tool(
                "observe",
                json!({ "workspace": "billing", "content": format!("Postgres is the {kind} the ledger lives in."),
                        "entities": [{ "name": "Postgres", "type": kind }] }),
            )
            .unwrap();
        obs.push(r["observation_id"].as_str().unwrap().to_string());
    }
    let g = graph(&d, "billing");
    let pg = node(&g, "Postgres").expect("Postgres");
    assert_eq!(pg["contested"], true, "two readings at one tier are contested: {pg}");

    let brief = app
        .rpc(
            "prompts/get",
            json!({ "name": "brief", "arguments": { "workspace": "billing" } }),
        )
        .unwrap();
    let text = brief["messages"].to_string();
    assert!(
        text.contains("contested") && text.contains("Database") && text.contains("Technology"),
        "{text}"
    );

    let (status, verdict) = d.viz(&format!(
        "/api/resolve?observation={}&tier=human_confirmed&workspace=billing",
        obs[1]
    ));
    assert_eq!(status, 200, "the console's verdict: {verdict}");
    let g =
        harness::wait_for("the verdict's projection", std::time::Duration::from_secs(10), || {
            let g = graph(&d, "billing");
            let pg = node(&g, "Postgres").cloned().ok_or("no Postgres")?;
            (pg["type"] == "Technology" && pg["trust_tier"] == "human_confirmed")
                .then_some(pg.clone())
                .ok_or(pg.to_string())
        });
    assert!(g.get("contested").is_none() || g["contested"] == false, "settled: {g}");
    let (_, explain) =
        d.viz(&format!("/api/explain?entity={}&workspace=billing", g["id"].as_str().unwrap()));
    assert!(
        explain.to_string().contains("Database"),
        "the other reading is kept, not erased: {explain}"
    );
}

/// A3. Merge, and take it back: an agent's proposal, accepted at the console, folds two spellings
/// into one; a split of that merge, accepted, restores two. The log keeps every step (P3).
#[test]
fn a3_merge_and_take_it_back() {
    let home = Home::new("a3");
    let d = Daemon::start(&home);
    let mut app = Mcp::connect(&d, "claude-code");
    for (name, content) in [
        ("Postgres", "Postgres holds the ledger."),
        ("PostgreSQL", "PostgreSQL backups run every six hours."),
    ] {
        app.tool("observe", json!({ "workspace": "billing", "content": content, "entities": [{ "name": name, "type": "Database" }] }))
            .unwrap();
    }
    let before = graph(&d, "billing");
    let id_of =
        |name: &str| node(&before, name).and_then(|n| n["id"].as_str()).expect(name).to_string();
    let (pg_id, pgsql_id) = (id_of("Postgres"), id_of("PostgreSQL"));
    let p = app
        .tool(
            "propose",
            json!({ "workspace": "billing", "kind": "entity_merge", "targets": [pg_id, pgsql_id],
                                 "into": pg_id, "rationale": "One database, two spellings." }),
        )
        .unwrap();
    let merge = p["proposal_id"]
        .as_str()
        .or(p["id"].as_str())
        .expect("a proposal id")
        .to_string();
    let (status, r) =
        d.viz(&format!("/api/review?proposal={merge}&decision=merge&workspace=billing"));
    assert_eq!(status, 200, "{r}");
    let merged = graph(&d, "billing");
    assert_eq!(merged["stats"]["node_count"], 1, "one node: {merged}");
    let pg = node(&merged, "Postgres").expect("the survivor");
    assert!(
        pg["aliases"].as_array().is_some_and(|a| a.iter().any(|x| x == "PostgreSQL")),
        "{pg}"
    );

    let (status, split) = d.viz(&format!("/api/propose_split?merge={merge}&workspace=billing"));
    assert_eq!(status, 200, "{split}");
    let split_id = split["proposal_id"].as_str().expect("a split proposal").to_string();
    let (status, r) =
        d.viz(&format!("/api/review?proposal={split_id}&decision=merge&workspace=billing"));
    assert_eq!(status, 200, "{r}");
    let restored = graph(&d, "billing");
    assert_eq!(restored["stats"]["node_count"], 2, "two again: {restored}");

    let (_, log) = d.viz("/api/observations?workspace=billing");
    assert!(
        log.as_array().map(Vec::len).unwrap_or(0) >= 6,
        "both observations and every proposal event are kept: {log}"
    );
}

/// A4. A restart forgets nothing, and a reprojection rebuilds the same graph (P16).
#[test]
fn a4_a_restart_forgets_nothing() {
    let home = Home::new("a4");
    let mut d = Daemon::start(&home);
    {
        let mut app = Mcp::connect(&d, "claude-code");
        for (i, (from, to)) in
            [("Invoice job", "Ledger"), ("Nightly export", "Ledger"), ("Ledger", "Postgres")]
                .iter()
                .enumerate()
        {
            app.tool(
                "observe",
                json!({ "workspace": "billing", "content": format!("fact {i}: {from} touches {to}"),
                        "entities": [{ "name": from, "type": "Job" }, { "name": to, "type": "Concept" }],
                        "relations": [{ "from": from, "type": "touches", "to": to }] }),
            )
            .unwrap();
        }
    }
    let shape = |g: &Value| {
        let mut ids: Vec<String> = g["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        (ids, g["stats"]["edge_count"].clone())
    };
    let before = shape(&graph(&d, "billing"));
    let status = d.stop().expect("an exit status");
    assert!(status.success(), "SIGTERM is a normal exit: {status}");

    let d2 = Daemon::restart(&home, &d);
    assert_eq!(shape(&graph(&d2, "billing")), before, "the same graph after a restart");
    drop(d2);

    let out = d.cli(&["reproject", "--workspace", "billing"]);
    assert!(
        out.status.success(),
        "reproject with the daemon stopped: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let d3 = Daemon::restart(&home, &d);
    assert_eq!(shape(&graph(&d3, "billing")), before, "the same graph after a reprojection");
}

/// A8. Refused, loudly: no token or another one is a 401 and runs nothing; a secret is refused at
/// ingest without being echoed; an unknown entity is unknown, not empty (P5, P24).
#[test]
fn a8_refused_loudly() {
    let home = Home::new("a8");
    let d = Daemon::start(&home);
    let init = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "x", "version": "1" } } })
    .to_string();
    let accept = ("Accept", "application/json, text/event-stream".to_string());
    let ctype = ("Content-Type", "application/json".to_string());
    let (none, _, _) = http(&d.addr, "POST", "/mcp", &[accept.clone(), ctype.clone()], &init);
    assert_eq!(none, 401, "no token");
    let (other, _, _) = http(
        &d.addr,
        "POST",
        "/mcp",
        &[accept, ctype, ("Authorization", "Bearer not-the-token".into())],
        &init,
    );
    assert_eq!(other, 401, "another token");

    let mut app = Mcp::connect(&d, "claude-code");
    let key = format!("AKIA{}", "Q7ZK3N5P2R8T4V6X");
    let refused = app.tool(
        "observe",
        json!({ "workspace": "ops", "content": format!("deploy with {key} from the vault") }),
    );
    let why = refused.expect_err("a secret is refused at ingest");
    assert!(!why.contains(&key), "the refusal does not echo the secret: {why}");
    let (_, log) = d.viz("/api/observations?workspace=ops");
    assert_eq!(log.as_array().map(Vec::len), Some(0), "nothing was written: {log}");

    let (status, body) = d.viz("/api/explain?entity=0000000000000000000000000000000000000000000000000000000000000000&workspace=ops");
    assert_eq!(status, 404, "an unknown entity is unknown: {body}");
}
