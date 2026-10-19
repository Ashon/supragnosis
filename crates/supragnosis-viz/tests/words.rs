//! docs/design-system.md U17: no line a person reads first spells a system identifier. These are
//! source tripwires over the viewer's script: a tier reaches the screen through `tierName`, never as
//! its identifier, and every proposal event the engine can write has a sentence in the Log, so a new
//! kind or event cannot arrive as "opened new_kind: 9f778c20".

const SCRIPT: &str = include_str!("../assets/viewer.js");
const ENGINE: &str = include_str!("../../supragnosis-engine/src/lib.rs");
const CORE: &str = include_str!("../../supragnosis-core/src/lib.rs");

const TIER_FIELDS: &[&str] = &["trust_tier", "effective_tier", "evaluated_tier"];

/// The text of every `${...}` interpolation in `src`, nested ones included, brackets counted.
fn interpolations(src: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(at) = rest.find("${") {
        let body = &rest[at + 2..];
        let mut depth = 1;
        let mut end = body.len();
        for (i, c) in body.char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push(&body[..end]);
        // Go on inside it: the viewer nests templates, `html\`${[html\`...${x}\`]}\``.
        rest = body;
    }
    out
}

/// The keys of a `const NAME = { key: ..., };` object literal of the viewer's script.
fn keys(name: &str) -> Vec<String> {
    let at = SCRIPT
        .find(&format!("const {name} = {{"))
        .unwrap_or_else(|| panic!("no {name}"));
    let body = &SCRIPT[at + name.len() + 10..];
    let body = &body[..body.find("};").expect("it closes")];
    body.split(',')
        .filter_map(|e| e.split(':').next())
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect()
}

#[test]
fn a_tier_is_named_never_spelled() {
    let mut raw = Vec::new();
    for expr in interpolations(SCRIPT) {
        let e = expr.trim();
        let bare = e.strip_prefix("String(").and_then(|e| e.strip_suffix(')')).unwrap_or(e);
        let last = bare.rsplit('.').next().unwrap_or(bare);
        if TIER_FIELDS.contains(&last) || bare == "tier" {
            raw.push(format!("${{{expr}}}"));
        }
    }
    // Nor is a tier's identifier joined into a string that is then drawn: `" -> " + String(tier)`.
    for field in TIER_FIELDS {
        for (at, _) in SCRIPT.match_indices(&format!("{field})")) {
            let Some(start) = SCRIPT[..at].rfind("String(") else {
                continue;
            };
            if SCRIPT[start + 7..at].contains(['(', ' ', ',']) {
                continue;
            }
            if SCRIPT[..start].trim_end().ends_with('+') {
                raw.push(SCRIPT[start..at + field.len() + 1].to_string());
            }
        }
    }
    assert!(
        raw.is_empty(),
        "a tier is drawn by its identifier, not tierName:\n  {}",
        raw.join("\n  ")
    );
    assert!(SCRIPT.contains("function tierName(t) { return TIER_NAME[String(t)] || String(t); }"));
    for (id, name) in [
        ("unverified", "Unverified"),
        ("agent_extracted", "Agent"),
        ("host_signed", "Host signed"),
        ("human_confirmed", "Human confirmed"),
    ] {
        assert!(SCRIPT.contains(&format!("{id}: \"{name}\"")), "TIER_NAME names {id} {name}");
    }
}

#[test]
fn every_proposal_event_has_a_sentence() {
    // The kinds the engine opens.
    let at = ENGINE.find("pub const PROPOSAL_KINDS").expect("PROPOSAL_KINDS");
    let list = &ENGINE[at..at + ENGINE[at..].find("];").expect("it closes")];
    let kinds: Vec<&str> = list.split('"').skip(1).step_by(2).collect();
    assert!(kinds.len() >= 6, "the kinds were read ({kinds:?})");
    let nouns = keys("PROPOSAL_NOUN");
    for k in &kinds {
        assert!(nouns.iter().any(|n| n == k), "the Log has no noun for {k}");
    }

    // The events a proposal can carry, and the decisions a verdict can take.
    let at = CORE.find("pub enum ProposalEventKind {").expect("ProposalEventKind");
    let body = &CORE[at..at + CORE[at..].find("\n}").expect("it closes")];
    let mut events: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter(|l| l.ends_with(',') && !l.starts_with("//"))
        .map(|l| l.trim_end_matches(',').to_lowercase())
        .collect();
    assert_eq!(events.len(), 4, "the events were read ({events:?})");
    events.retain(|e| e != "verdict");
    let decisions = &ENGINE[ENGINE.find("fn review_proposal(").expect("review_proposal")..];
    let decisions = &decisions[..decisions.find("=> ProposalEventKind::Verdict").expect("verdict")];
    let decisions = decisions.rsplit('\n').next().expect("the decision arm");
    events.extend(decisions.split('"').skip(1).step_by(2).map(str::to_string));
    let verbs = keys("EVENT_VERB");
    for e in &events {
        assert!(verbs.iter().any(|v| v == e), "the Log has no verb for {e}");
    }
    assert!(events.contains(&"merge".to_string()) && events.contains(&"reject".to_string()));

    // The line is built from those tables, and a row it cannot phrase says so in words.
    let line = &SCRIPT[SCRIPT.find("function proposalLine(").expect("proposalLine")..];
    let line = &line[..line.find("\n}\n").expect("it closes")];
    assert!(line.contains("PROPOSAL_NOUN[p.kind]") && line.contains("EVENT_VERB["));
    assert!(line.contains("\"Something changed\""));
    assert!(!line.contains("slice(0, 8)"), "a name is cut short");
}
