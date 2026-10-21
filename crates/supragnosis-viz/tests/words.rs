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

/// Every Log row says who and when under what happened: the tier by its name, the writer as the strip
/// names one, and the time once per minute; its mark is the ring the map draws for its tier.
#[test]
fn every_log_row_says_who_and_when() {
    let at = SCRIPT.find("function obsRowHtml(").expect("obsRowHtml");
    let row = &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")];
    for part in [
        "tierName(o.effective_tier)",
        "writerFor(a0)",
        "obsWhen(prev.observed_at) === when",
    ] {
        assert!(row.contains(part), "a Log row does not say {part}");
    }
    let at = SCRIPT.find("function tierDot(").expect("tierDot");
    let mark = &SCRIPT[at..at + SCRIPT[at..].find('\n').expect("one line")];
    assert!(
        mark.contains("class=\"tsw tmark t-"),
        "a row's tier mark is not the legend's ring"
    );
}

/// A source with its comments left out: `//` to the line's end where whitespace or the line's start
/// comes before it (so an address's `://` stays), `/* ... */`, and `<!-- ... -->`.
fn prose(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    loop {
        let block = rest.find("/*");
        let html = rest.find("<!--");
        let at = match (block, html) {
            (Some(b), Some(h)) => b.min(h),
            (Some(b), None) => b,
            (None, Some(h)) => h,
            (None, None) => break,
        };
        out.push_str(&rest[..at]);
        let close = if rest[at..].starts_with("/*") { "*/" } else { "-->" };
        rest = match rest[at..].find(close) {
            Some(end) => &rest[at + end + close.len()..],
            None => "",
        };
    }
    out.push_str(rest);
    out.lines()
        .map(|l| {
            let cut = l
                .match_indices("//")
                .find(|(i, _)| *i == 0 || l[..*i].ends_with(char::is_whitespace))
                .map(|(i, _)| i);
            match cut {
                Some(i) => &l[..i],
                None => l,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A Rust source without its `#[cfg(test)]` items: each runs from the attribute to the next line that
/// rustfmt starts with a closing `}` or `]` in the first column - a test module's end, or a test-only
/// list's.
fn without_tests(src: &str) -> String {
    let mut out = String::new();
    let mut rest = src;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let item = &rest[at..];
        let end = ["\n}", "\n]"]
            .iter()
            .filter_map(|close| item.find(close).map(|i| i + close.len()))
            .min()
            .unwrap_or(item.len());
        rest = &item[end..];
    }
    out.push_str(rest);
    out
}

/// docs/design-system.md Section 9 and U17: a retired word does not come back. The table's left
/// column, as it was written, is looked for in every page's markup and script, the shell's lines and
/// Setup's sentences - the code before each file's tests, comments left out.
#[test]
fn no_retired_word_reaches_a_page() {
    let doc = include_str!("../../../docs/design-system.md");
    let at = doc.find("**Retired words**").expect("Section 9 lists the retired words");
    let table = &doc[at..];
    let table = &table[table.find("| Retired |").expect("its table")..];
    let retired: Vec<&str> = table
        .lines()
        .skip(2)
        .take_while(|l| l.starts_with('|'))
        .map(|l| l.trim_start_matches('|').split('|').next().unwrap_or("").trim())
        .filter(|w| !w.is_empty())
        .collect();
    assert!(retired.len() >= 20, "the table was not read: {retired:?}");
    let before_tests = |src: &'static str| without_tests(src);
    let sources: [(&str, String); 11] = [
        ("viewer.html", include_str!("../assets/viewer.html").to_string()),
        ("viewer.js", SCRIPT.to_string()),
        ("app/assets/kit.js", include_str!("../../../app/assets/kit.js").to_string()),
        (
            "app/assets/settings.js",
            include_str!("../../../app/assets/settings.js").to_string(),
        ),
        (
            "app/assets/settings.html",
            include_str!("../../../app/assets/settings.html").to_string(),
        ),
        (
            "app/assets/welcome.js",
            include_str!("../../../app/assets/welcome.js").to_string(),
        ),
        (
            "app/assets/welcome.html",
            include_str!("../../../app/assets/welcome.html").to_string(),
        ),
        (
            "app/assets/shell-init.js",
            include_str!("../../../app/assets/shell-init.js").to_string(),
        ),
        ("app/src/main.rs", before_tests(include_str!("../../../app/src/main.rs"))),
        (
            "supragnosis-cli/src/setup.rs",
            before_tests(include_str!("../../supragnosis-cli/src/setup.rs")),
        ),
        // The viewer's server: a refusal it writes reaches a notice.
        ("supragnosis-viz/src/lib.rs", before_tests(include_str!("../src/lib.rs"))),
    ];
    let mut found = Vec::new();
    for (name, src) in sources {
        let text = prose(&src);
        for word in &retired {
            if text.contains(word) {
                found.push(format!("{name}: \"{word}\""));
            }
        }
    }
    assert!(found.is_empty(), "retired words are back on a page: {found:#?}");
}

/// docs/design-system.md Section 29.6 and U17: the map's passing activity is named and speaks in
/// sentences - no tool name, abbreviation or mode id - and the status bar's footprint says what
/// agents used, not a session's id.
#[test]
fn the_maps_activity_speaks_in_sentences() {
    let html = include_str!("../assets/viewer.html");
    assert!(html.contains("<div id=\"log\" role=\"region\" aria-label=\"Activity\">"));
    let at = SCRIPT.find("async function handleEvent(").expect("handleEvent");
    let handle = &SCRIPT[at..];
    let handle = &handle[..handle.find("\n}\n").expect("it closes")];
    let rows: Vec<&str> = handle
        .match_indices("logRow(")
        .map(|(i, _)| handle[i..].lines().next().unwrap_or(""))
        .collect();
    assert!(rows.len() >= 5, "the rows were not found: {rows:?}");
    for row in &rows {
        for spelled in ["<b>", " ent,", " rel ", "ev.mode", "get_entity", "ws ${", "&lt;-&gt;"] {
            assert!(!row.contains(spelled), "an activity row spells {spelled:?}: {row}");
        }
    }
    let body = |name: &str| {
        let at = SCRIPT.find(&format!("function {name}(")).unwrap_or_else(|| panic!("no {name}"));
        let f = &SCRIPT[at..];
        f[..f.find("\n}\n").expect("it closes")].to_string()
    };
    let all =
        [body("wroteSaid"), body("syncSaid"), body("activitySaid"), handle.to_string()].concat();
    for said in [
        "actorOf(ev) + \" wrote \"",
        "actorOf(ev) + \" searched \\\"\"",
        "actorOf(ev) + \" looked up \"",
        "actorOf(ev) + \" followed \"",
        "\" sent \"",
        "\" read \"",
    ] {
        assert!(all.contains(said), "the activity does not say {said}");
    }
    // Who acted is the app the local event names, else "An agent" (design-system.md Section 29.8).
    assert!(SCRIPT.contains("function actorOf(ev) { return ev.app || \"An agent\"; }"));
    assert!(
        SCRIPT.contains("return a.app && mine ? a.app + \" on \" + machine : machine;"),
        "the who-line does not name the app"
    );
    assert!(handle.contains("\"Agents used \" + plural(footprint.size, \"entity\", \"entities\")"));
    assert!(!handle.contains("`session ${"), "the status bar names the session by its id");
}

/// docs/design-system.md Section 29.7 and U17: one who-line grammar - tier, writer, for whom, when -
/// from the attestation that wrote it, where "here" is a sync origin and never a host name, and the
/// card resolves every id: Cited first, the act that set its tier, Derived from and Open in Review.
#[test]
fn a_who_line_names_the_writer_apart_from_whom_it_wrote_for() {
    let body = |name: &str| {
        let at = SCRIPT.find(&format!("function {name}(")).unwrap_or_else(|| panic!("no {name}"));
        let f = &SCRIPT[at..];
        f[..f.find("\n}\n").unwrap_or_else(|| f.find("}\n").expect("it closes"))].to_string()
    };
    let writer = body("writerOf");
    for part in [
        "writtenHere(a)",
        "\"you at this console\"",
        "\"a person at \"",
        "String(a.origin_node).slice(0, 8)",
    ] {
        assert!(writer.contains(part), "writerOf does not say {part}");
    }
    assert!(!writer.contains("a.host"), "a host name decides here");
    assert!(SCRIPT.contains("function writtenHere(a) { return !a.origin_node || (!!ownNode && a.origin_node === ownNode); }"));
    assert!(SCRIPT.contains("(a.on_behalf_of ? \", for \" + a.on_behalf_of : \"\")"));
    assert!(
        SCRIPT.contains("return as[o && o.author | 0] || as[0] || null;"),
        "the row names the first by host"
    );
    assert!(body("obsRowHtml").contains("authorOf(o)"));
    let card = body("obsCardHtml");
    for part in [
        "!/^surface:/.test(r)",
        "/^https?:\\/\\//.test(cited)",
        "Cited: ",
        "o.gated_by",
        "\"Lowered to \"",
        "the agent's confidence: ",
        "data-open-obs=",
        "data-open-review=",
        "As stored",
    ] {
        assert!(card.contains(part), "the card does not hold {part}");
    }
    assert!(
        body("observationById").contains("/api/observations?id="),
        "an id read loads the scope's log"
    );
    let settle = body("fillSettleWho");
    assert!(
        settle.contains("firstSentence(o.content)")
            && settle.contains("whoLine(\"\", authorOf(o), shared)")
    );
    let rows = body("contestedRows");
    assert!(rows.contains("\"In Supragnosis\"") && rows.contains("i-chevron-right"));
    assert!(
        body("relationGroupHtml").contains("chipTier(e.trust_tier)"),
        "a relation draws no trust"
    );
    assert!(body("waitsHtml").contains("Waits for you: "));
}

/// design-system.md Section 29.8: one table of the AI apps' names. The core crate holds it; the app's
/// pages (`kit.js`) and its shell (`main.rs`) keep copies, which say the same, app for app.
#[test]
fn the_app_names_are_one_table() {
    let kit = include_str!("../../../app/assets/kit.js");
    let shell = include_str!("../../../app/src/main.rs");
    for (id, name) in supragnosis_core::AI_APPS {
        let key = if id.contains('-') {
            format!("\"{id}\": \"{name}\"")
        } else {
            format!("{id}: \"{name}\"")
        };
        assert!(kit.contains(&key), "kit.js does not name {id} {name}");
        assert!(
            shell.contains(&format!("(\"{id}\", \"{name}\")")),
            "the shell does not name {id} {name}"
        );
    }
}
