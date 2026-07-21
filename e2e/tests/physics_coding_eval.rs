//! Physics coding eval: does a shared-ontology delegation improve a small model's "coding output".
//!
//! Unlike delegation_eval (recall QA) and ontology_build_eval (ingestion), this harness directly
//! measures the ultimate purpose of delegation - better work output. It preloads a high-quality
//! shared ontology curated by a human (the harness author) ("the team's design decisions") into
//! the knowledge base, and has the model implement a 2D physics demo (self-contained HTML) that
//! runs in a browser:
//!
//!   - bare      condition: give only the task. The design decisions are unknown.
//!   - delegated condition: the same task + MCP tools. Instruct it that "the team design is in the
//!     knowledge base - look it up and follow it".
//!
//! Into the design decisions we plant concrete numbers/formulas that exist only in the knowledge
//! base (gravity 900, restitution 0.8, damping 0.999, dt 1/60, the (1+e) term of the impulse
//! formula, invMass, positional correction). If these fingerprints appear in the generated code,
//! it is direct evidence that the knowledge flowed through MCP into the code - the bare condition
//! cannot match them in principle (chance matches show up as a base rate).
//!
//! Quantitative metrics (model x condition):
//!   - hit count of the 7 design-conformance fingerprints (the core signal of knowledge delegation -
//!     but a measure of "the knowledge was carried over", not of "the code is correct". Measured:
//!     a case with 7/7 conformance where the balls did not move)
//!   - behavior scoring: judges movement/bounce/containment from the trajectory drawn by a headless run
//!   - JS syntax validity (node --check, skipped if node is absent)
//!   - output-format conformance (a single html code block), tool-call count, tokens
//!
//! EVAL_REPLAY=1 re-scores the saved demo files without model calls (deterministic).
//! EVAL_REPAIR_ROUNDS (default 2): the number of repair rounds that, on failure, feed the failure
//! detail (syntax stderr, runtime error, behavior shortfall) back into the same conversation to
//! have it fixed. The metric is how much the success rate improves over the initial generation -
//! whether delegation + a feedback loop compensates for the assembly limits of small models.
//! Qualitative artifacts:
//!   - target/eval-reports/physics_demos/{model}_{condition}.html - the actual runnable demo
//!   - target/eval-reports/physics_gallery.html - a gallery placing all demos side by side
//!     (iframe srcdoc embeds - self-contained, opens anywhere locally/on the web)
//!
//! Run:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test physics_coding_eval -- --ignored --nocapture
//! Optional env: OLLAMA_BASE_URL (default http://localhost:11434), OLLAMA_MODELS (default gemma4)
//!
//! If Ollama is not up, it silently passes (skips).

use std::sync::Arc;
use std::time::Duration;

use rmcp::service::RunningService;
use rmcp::RoleClient;
use serde_json::{json, Value};

use supragnosis_e2e::bridge::{
    chat, exec_tool, ollama_reachable, openai_tools, push_message, serve_engine, tool_calls,
    DEFAULT_BASE,
};
use supragnosis_e2e::report::{self, report_dir};
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::{Engine, EntityInput, ObserveInput, RelationInput};
use supragnosis_store::InMemoryStore;

const DEFAULT_MODELS: &str = "gemma4";
const WS: &str = "physics";
const MAX_ROUNDS: usize = 8;

// --- Shared ontology (team design decisions curated by a human) --------------

/// A single design decision. content is the body of the knowledge; entities/relations are the ontology skeleton.
struct Decision {
    content: &'static str,
    entities: &'static [(&'static str, &'static str)],
    relations: &'static [(&'static str, &'static str, &'static str)],
}

/// The team's physics-demo design decisions. Holds every number/formula/order needed to implement
/// it - this content is never placed in the prompt, and only the delegated-condition model can
/// obtain it via MCP lookup.
fn design_decisions() -> Vec<Decision> {
    vec![
        Decision {
            content: "world step order per frame: apply gravity, integrate all bodies, detect \
                      circle collisions, resolve impulses, correct positions; use a fixed \
                      timestep dt = 1/60 second",
            entities: &[("world step", "Process"), ("fixed timestep", "Concept")],
            relations: &[("world step", "uses", "fixed timestep")],
        },
        Decision {
            content: "integration is semi-implicit euler: velocity += acceleration * dt first, \
                      then position += velocity * dt; never update position before velocity",
            entities: &[("semi-implicit euler", "Algorithm")],
            relations: &[("world step", "uses", "semi-implicit euler")],
        },
        Decision {
            content: "gravity acceleration is 900 pixels per second squared, downward (+y) in \
                      canvas coordinates",
            entities: &[("gravity", "Force")],
            relations: &[("world step", "applies", "gravity")],
        },
        Decision {
            content: "each ball is a circle body: radius random 12 to 24, mass = radius * radius, \
                      inverse mass invMass = 1 / mass; spawn 25 balls at random non-overlapping \
                      positions",
            entities: &[("ball", "Component"), ("invMass", "Quantity")],
            relations: &[("ball", "has", "invMass")],
        },
        Decision {
            content: "circle vs circle collision: colliding when distance between centers < sum \
                      of radii; contact normal = normalized center difference; penetration = sum \
                      of radii - distance",
            entities: &[("circle collision test", "Algorithm"), ("contact", "Concept")],
            relations: &[("circle collision test", "produces", "contact")],
        },
        Decision {
            content: "impulse resolution: relative velocity rv = vB - vA; velAlongNormal = \
                      dot(rv, normal); skip if velAlongNormal > 0 (separating); impulse magnitude \
                      j = -(1 + restitution) * velAlongNormal / (invMassA + invMassB); then \
                      vA -= j * normal * invMassA and vB += j * normal * invMassB",
            entities: &[("impulse resolution", "Algorithm"), ("restitution", "Quantity")],
            relations: &[
                ("impulse resolution", "consumes", "contact"),
                ("impulse resolution", "uses", "restitution"),
            ],
        },
        Decision {
            content: "restitution is 0.8 for ball-ball collisions and 0.9 for wall bounces",
            entities: &[("restitution", "Quantity")],
            relations: &[],
        },
        Decision {
            content: "positional correction to prevent sinking: move each body along the contact \
                      normal by 0.6 * max(penetration - 0.01, 0) / (invMassA + invMassB) * its \
                      own invMass",
            entities: &[("positional correction", "Algorithm")],
            relations: &[("positional correction", "consumes", "contact")],
        },
        Decision {
            content: "canvas boundaries are walls: clamp ball position inside and reflect the \
                      velocity component, scaled by wall restitution 0.9",
            entities: &[("wall bounce", "Algorithm")],
            relations: &[("wall bounce", "uses", "restitution")],
        },
        Decision {
            content: "rendering: html canvas 800 x 500, dark background #14161a, one \
                      requestAnimationFrame loop that steps the world then draws each ball as a \
                      filled circle with a distinct color",
            entities: &[("renderer", "Component")],
            relations: &[("renderer", "draws", "ball")],
        },
        Decision {
            content: "linear damping: multiply each velocity by 0.999 every step to keep the \
                      simulation stable",
            entities: &[("linear damping", "Algorithm")],
            relations: &[("world step", "uses", "linear damping")],
        },
    ]
}

/// Design fingerprints: if this pattern appears in the code, that design decision flowed into it.
/// (name, regex) - chosen as concrete numbers/structures the bare condition is unlikely to match by chance.
const FINGERPRINTS: [(&str, &str); 7] = [
    ("gravity 900", r"900"),
    ("restitution 0.8", r"0\.8"),
    ("damping 0.999", r"0\.999"),
    ("dt 1/60", r"1\s*/\s*60|0\.01666|0\.0167"),
    ("impulse (1+e)", r"\(\s*1(\.0)?\s*\+\s*[a-zA-Z_.]*(e|rest)"),
    ("invMass", r"(?i)inv_?mass"),
    ("positional correction", r"(?i)penetrat|correction"),
];

// --- Ollama bridge (same pattern as delegation_eval.rs) ----------------------

// --- Task and execution ------------------------------------------------------

const TASK: &str = "Implement a small 2D physics demo as ONE self-contained HTML file: balls \
    bouncing under gravity inside a canvas, with ball-to-ball collisions properly resolved. \
    No external libraries. Reply with a single ```html code block containing the complete file \
    and nothing else after it.";

/// Additional instruction appended after TASK in the delegated condition (instruction to query the knowledge base).
const DELEGATED_SUFFIX: &str = "\n\nIMPORTANT: your team's agreed design decisions for this \
demo (step order, integration method, exact constants, collision and impulse formulas, \
rendering spec) are stored in the knowledge base. Before writing code, use the \
search_knowledge tool (several queries, e.g. \"step order\", \"impulse\", \"restitution\", \
\"gravity\", \"rendering\") to retrieve the design, then follow it exactly in your \
implementation.";

/// Repair-round feedback template. {issues} = list of failure items found by the automatic scorer.
const FEEDBACK_TEMPLATE: &str = "Your demo was tested automatically and it FAILED:\n- \
{issues}\n\nFix the problem and reply again with ONE complete ```html code block containing \
the whole corrected file (not a diff).";

/// Result of a single (model, condition) run. If there are repair rounds, the final round's evaluation is carried.
struct CodeResult {
    model: String,
    condition: &'static str,
    /// The extracted HTML (code block). Absent means a format violation.
    html: Option<String>,
    fingerprints: Vec<(&'static str, bool)>,
    syntax_ok: Option<bool>, // None = no node or no html
    /// Behavior scoring (headless run). None = no node or no html.
    behavior: Option<Behavior>,
    search_calls: usize,
    tokens: u64,
    error: Option<String>,
    /// Per-round verdict labels (r0 = initial generation, r1+ = repair rounds).
    rounds: Vec<String>,
    /// Whether the initial generation (r0) succeeded - the baseline for repair gain.
    initial_success: bool,
    /// Final success: html + syntax + behavior (runs/moves/bounces/contained) all pass.
    success: bool,
}

impl CodeResult {
    fn fp_hits(&self) -> usize {
        self.fingerprints.iter().filter(|(_, h)| *h).count()
    }

    /// Round summary in the form "succeeded at 2/3" / "3/3 failed".
    fn rounds_label(&self) -> String {
        if self.success {
            format!("succeeded at {}/{}", self.rounds.len(), self.rounds.len())
        } else {
            format!("{}/{} failed", self.rounds.len(), self.rounds.len())
        }
    }
}

/// Extracts an html code block (or via a doctype heuristic) from the response text.
fn extract_html(text: &str) -> Option<String> {
    // Prefer ```html ... ```; failing that, accept ``` ... ``` if it contains <canvas.
    for marker in ["```html", "```HTML", "```"] {
        if let Some(start) = text.find(marker) {
            let body_start = start + marker.len();
            if let Some(end_rel) = text[body_start..].find("```") {
                let body = text[body_start..body_start + end_rel].trim();
                if body.contains("<canvas") || body.contains("<!doctype") || body.contains("<!DOCTYPE")
                {
                    return Some(body.to_string());
                }
            }
        }
    }
    // Accept models that spit out raw html with no code block.
    if text.contains("<canvas") && (text.contains("<html") || text.contains("<!doctype") || text.contains("<!DOCTYPE")) {
        return Some(text.trim().to_string());
    }
    None
}

/// Extracts and concatenates all script bodies from the html. None if there is no script tag or it is broken.
fn extract_script(html: &str) -> Option<String> {
    let mut script = String::new();
    let mut rest = html;
    while let Some(s) = rest.find("<script") {
        let after = &rest[s..];
        let open_end = after.find('>')?;
        let body = &after[open_end + 1..];
        let close = body.find("</script>")?;
        script.push_str(&body[..close]);
        script.push('\n');
        rest = &body[close..];
    }
    if script.trim().is_empty() {
        None
    } else {
        Some(script)
    }
}

/// Syntax-checks the script body with node --check. None if node is absent.
/// (ok, on failure the head of stderr) - the stderr is used for repair-round feedback.
fn check_syntax(html: &str, tmp: &std::path::Path) -> Option<(bool, String)> {
    let Some(script) = extract_script(html) else {
        return Some((false, "no <script> body found".into())); // A "demo" with no script fails.
    };
    let path = tmp.join("syntax_check.js");
    std::fs::write(&path, &script).ok()?;
    std::process::Command::new("node")
        .arg("--check")
        .arg(&path)
        .output()
        .ok()
        .map(|o| {
            let err: String = String::from_utf8_lossy(&o.stderr).chars().take(300).collect();
            (o.status.success(), err)
        })
}

// --- Behavior scoring (runtime scoring) --------------------------------------
//
// The fingerprint check only measures "was the knowledge carried into the code" - assembly-logic
// bugs (order of application, unit confusion) only surface when run (measured: gemma4 delegated at
// 7/7 conformance reset gravity right after applying it, so the balls did not move). So we stub
// the DOM/canvas in node and record ctx.arc(x,y,r) calls per frame, scoring behavior from "the
// drawn balls' trajectory" independent of the demo's internal implementation.

/// Headless runner for node. Drives rAF/setInterval for 3600 frames (equivalent to 60 seconds),
/// collecting arc coordinates and printing them as JSON. Unknown DOM accesses are absorbed by a
/// black-hole proxy. Why the observation window is 60 seconds (measured): generated code with the
/// wrong time scale takes tens of seconds to fall - in a 4-second window a demo that "bounces,
/// however slowly" was misjudged as stationary.
const RUNNER_JS: &str = r##"
const fs = require('fs');
const W = 800, H = 500;
const frames = [];
let current = [];
const hole = new Proxy(function () {}, {
  get(t, p) { if (p === Symbol.toPrimitive) return () => 0; return hole; },
  apply() { return hole; },
  set() { return true; },
  construct() { return hole; },
});
const canvasStub = {
  width: W, height: H, style: {},
  addEventListener() {},
  getBoundingClientRect() { return { left: 0, top: 0, width: W, height: H }; },
};
const ctxStub = new Proxy({}, {
  get(t, p) {
    if (p === 'arc') return (x, y, r) => { current.push([+x, +y, +r]); };
    if (p === 'canvas') return canvasStub;
    return hole;
  },
  set() { return true; },
});
canvasStub.getContext = () => ctxStub;
globalThis.document = new Proxy({}, {
  get(t, p) {
    if (p === 'getElementById' || p === 'querySelector' || p === 'createElement') {
      return () => canvasStub;
    }
    if (p === 'addEventListener') return () => {};
    return hole;
  },
  set() { return true; },
});
globalThis.window = globalThis;
globalThis.innerWidth = W;
globalThis.innerHeight = H;
let rafCb = null;
const intervalCbs = [];
globalThis.requestAnimationFrame = cb => { rafCb = cb; return 0; };
globalThis.cancelAnimationFrame = () => {};
globalThis.setInterval = cb => { intervalCbs.push(cb); return 0; };
globalThis.setTimeout = cb => { intervalCbs.push(cb); return 0; };
globalThis.clearInterval = () => {};
globalThis.clearTimeout = () => {};
let t = 0;
globalThis.performance = { now: () => t };
globalThis.addEventListener = () => {};
function emit(extra) {
  console.log(JSON.stringify(Object.assign(
    { frames, width: canvasStub.width, height: canvasStub.height }, extra || {})));
}
const src = fs.readFileSync(process.argv[2], 'utf8');
try { (0, eval)(src); } catch (e) { emit({ error: String(e) }); process.exit(0); }
frames.push(current);
for (let i = 0; i < 3600; i++) {
  const cb = rafCb; rafCb = null;
  current = [];
  t += 1000 / 60;
  try {
    if (cb) cb(t);
    for (const f of intervalCbs) f();
  } catch (e) { emit({ error: String(e) }); process.exit(0); }
  frames.push(current);
  if (!rafCb && intervalCbs.length === 0) break;
}
emit();
"##;

/// Behavior scoring result. `ran=false` means the run itself failed (runtime error / too few frames).
#[derive(Clone)]
struct Behavior {
    ran: bool,
    /// Whether the balls actually moved (median y range of movement > 15px).
    moved: bool,
    /// Whether 95%+ of the samples are inside the canvas (60px margin).
    contained: bool,
    /// Time of the first observed bounce (simulation seconds). Per the spec it should be within 1 second.
    first_bounce_s: Option<f64>,
    note: String,
}

impl Behavior {
    fn label(&self) -> String {
        if !self.ran {
            return format!("run failed ({})", self.note);
        }
        let bounce = match self.first_bounce_s {
            Some(s) => format!("[o] ~{s:.1}s"),
            None => "[x]".to_string(),
        };
        format!(
            "movement {} / bounce {bounce} / containment {}",
            if self.moved { "[o]" } else { "[x]" },
            if self.contained { "[o]" } else { "[x]" }
        )
    }
}

/// Runs the runner to obtain per-frame arc coordinates and computes behavior metrics. None if node
/// is absent. The runner's frame loop is bounded, but a 10-second timeout guards against generated
/// code's own infinite loops.
fn run_behavior(html: &str, tmp: &std::path::Path) -> Option<Behavior> {
    let script = match extract_script(html) {
        Some(s) => s,
        None => {
            return Some(Behavior {
                ran: false,
                moved: false,
                contained: false,
                first_bounce_s: None,
                note: "no script".into(),
            })
        }
    };
    let runner = tmp.join("behavior_runner.js");
    let target = tmp.join("behavior_target.js");
    std::fs::write(&runner, RUNNER_JS).ok()?;
    std::fs::write(&target, &script).ok()?;

    let mut child = std::process::Command::new("node")
        .arg(&runner)
        .arg(&target)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // Timeout using only std: a separate thread does the reading, and the main line waits up to 10 seconds.
    let (tx, rx) = std::sync::mpsc::channel();
    let mut stdout = child.stdout.take()?;
    std::thread::spawn(move || {
        use std::io::Read as _;
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    let out = match rx.recv_timeout(Duration::from_secs(10)) {
        Ok(buf) => {
            let _ = child.wait();
            buf
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return Some(Behavior {
                ran: false,
                moved: false,
                contained: false,
                first_bounce_s: None,
                note: "timeout (suspected infinite loop)".into(),
            });
        }
    };
    let v: Value = serde_json::from_str(out.trim()).ok()?;
    Some(score_behavior(&v))
}

fn score_behavior(v: &Value) -> Behavior {
    let fail = |note: &str| Behavior {
        ran: false,
        moved: false,
        contained: false,
        first_bounce_s: None,
        note: note.into(),
    };
    if let Some(e) = v.get("error").and_then(Value::as_str) {
        let head: String = e.chars().take(80).collect();
        return fail(&head);
    }
    let frames: Vec<Vec<(f64, f64, f64)>> = v["frames"]
        .as_array()
        .map(|fs| {
            fs.iter()
                .map(|f| {
                    f.as_array()
                        .map(|balls| {
                            balls
                                .iter()
                                .filter_map(|b| {
                                    let a = b.as_array()?;
                                    Some((a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?))
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default();
    let (w, h) = (
        v["width"].as_f64().unwrap_or(800.0),
        v["height"].as_f64().unwrap_or(500.0),
    );

    // Use only the frames with the most common ball count as the trajectory (removing init/effect-frame noise).
    let nb = frames.iter().map(Vec::len).max().unwrap_or(0);
    let stable: Vec<&Vec<(f64, f64, f64)>> =
        frames.iter().filter(|f| f.len() == nb && nb > 0).collect();
    if stable.len() < 10 {
        return fail("too few frames (animation loop not running)");
    }

    let mut ranges = Vec::new();
    let mut bounce_frame: Option<usize> = None;
    for ball in 0..nb {
        let ys: Vec<f64> = stable.iter().map(|f| f[ball].1).collect();
        let (min, max) = ys
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
        ranges.push(max - min);
        // A fall (cumulative +30px) followed by a rise (-15px) = a bounce. Record the first
        // observed frame (for diagnosing time-scale bugs - per spec within 1 second, tens of
        // seconds suggests a shrunken dt).
        let (mut peak_fall, mut lowest) = (0.0f64, ys[0]);
        let mut start = ys[0];
        for (idx, &y) in ys.iter().enumerate().skip(1) {
            if y > lowest {
                lowest = y;
                peak_fall = peak_fall.max(lowest - start);
            }
            if peak_fall > 30.0 && lowest - y > 15.0 {
                bounce_frame = Some(match bounce_frame {
                    Some(f) => f.min(idx),
                    None => idx,
                });
                break;
            }
            if y < start {
                start = y;
                lowest = y;
            }
        }
    }
    ranges.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let moved = ranges.get(ranges.len() / 2).copied().unwrap_or(0.0) > 15.0;

    let (mut inside, mut total) = (0usize, 0usize);
    for f in &stable {
        for (x, y, _) in f.iter() {
            total += 1;
            if *x >= -60.0 && *x <= w + 60.0 && *y >= -60.0 && *y <= h + 60.0 {
                inside += 1;
            }
        }
    }
    let contained = total > 0 && (inside as f64 / total as f64) >= 0.95;

    let bounce_note = match bounce_frame {
        Some(f) => format!(", first bounce ~{:.1}s", f as f64 / 60.0),
        None => String::new(),
    };
    Behavior {
        ran: true,
        moved,
        contained,
        first_bounce_s: bounce_frame.map(|f| f as f64 / 60.0),
        note: format!("frames {} balls {nb}{bounce_note}", stable.len()),
    }
}

/// Evaluation of a single round's output: format/fingerprints/syntax/behavior.
struct Evaluation {
    html: Option<String>,
    fingerprints: Vec<(&'static str, bool)>,
    syntax: Option<(bool, String)>,
    behavior: Option<Behavior>,
}

impl Evaluation {
    /// Success = html present + syntax passes (when measurable) + all behavior items (runs/moves/bounces/contained).
    fn success(&self) -> bool {
        self.html.is_some()
            && !matches!(self.syntax, Some((false, _)))
            && self
                .behavior
                .as_ref()
                .is_some_and(|b| b.ran && b.moved && b.first_bounce_s.is_some() && b.contained)
    }

    /// A short verdict label for the round log.
    fn label(&self) -> String {
        if self.html.is_none() {
            return "no html".into();
        }
        if let Some((false, _)) = &self.syntax {
            return "syntax error".into();
        }
        match &self.behavior {
            Some(b) if !b.ran => format!("run failed ({})", b.note),
            Some(b) if self.success() => {
                format!("success (bounce ~{:.1}s)", b.first_bounce_s.unwrap_or(0.0))
            }
            Some(b) => format!(
                "behavior shortfall (movement {} bounce {} containment {})",
                if b.moved { "o" } else { "x" },
                if b.first_bounce_s.is_some() { "o" } else { "x" },
                if b.contained { "o" } else { "x" }
            ),
            None => "behavior not measured".into(),
        }
    }

    /// Repair-round feedback: spells out the failure items concretely so it can fix them.
    fn feedback(&self) -> String {
        let mut issues: Vec<String> = Vec::new();
        if self.html.is_none() {
            issues.push(
                "your reply did not contain a single complete ```html code block".into(),
            );
        }
        if let Some((false, err)) = &self.syntax {
            issues.push(format!("the JavaScript has a syntax error:\n{err}"));
        } else if let Some(b) = &self.behavior {
            if !b.ran {
                issues.push(format!("the script crashes at runtime: {}", b.note));
            } else {
                if !b.moved {
                    issues.push(
                        "the balls never move - make sure gravity actually reaches velocity \
                         and velocity reaches position every frame (check units and reset \
                         ordering)"
                            .into(),
                    );
                }
                if b.first_bounce_s.is_none() {
                    issues.push(
                        "the balls never bounce off the floor within 60 simulated seconds - \
                         check the gravity magnitude / time step scale"
                            .into(),
                    );
                } else if !b.contained {
                    issues.push("balls escape the canvas - clamp positions at the walls".into());
                }
            }
        }
        FEEDBACK_TEMPLATE.replace("{issues}", &issues.join("\n- "))
    }
}

fn evaluate(text: &str, tmp: &std::path::Path) -> Evaluation {
    let html = extract_html(text);
    let fingerprints = FINGERPRINTS
        .iter()
        .map(|(name, pat)| {
            let hit = html
                .as_deref()
                .map(|h| regex_lite::Regex::new(pat).unwrap().is_match(h))
                .unwrap_or(false);
            (*name, hit)
        })
        .collect();
    let syntax = html.as_deref().and_then(|h| check_syntax(h, tmp));
    let behavior = html.as_deref().and_then(|h| run_behavior(h, tmp));
    Evaluation {
        html,
        fingerprints,
        syntax,
        behavior,
    }
}

/// Obtains a single assistant text answer from the conversation history.
/// delegated (tools=Some) executes and feeds back tool_calls, looping until text appears.
/// The received assistant message is pushed onto the history so repair rounds continue the same conversation.
async fn next_answer(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    messages: &mut Value,
    bridge: Option<(&RunningService<RoleClient, ()>, &Value)>,
    search_calls: &mut usize,
    tokens_sum: &mut u64,
) -> Result<String, String> {
    let tools = bridge.map(|(_, t)| t);
    for _ in 0..MAX_ROUNDS {
        let (msg, usage) = chat(http, base, model, &*messages, tools).await?;
        *tokens_sum += usage.total();
        let calls = tool_calls(&msg);
        push_message(messages, msg.clone());
        if calls.is_empty() {
            return Ok(msg.get("content").and_then(Value::as_str).unwrap_or("").to_string());
        }
        let Some((client, _)) = bridge else {
            return Err("tool_calls occurred in a no-tools condition".into());
        };
        for (id, name, args) in &calls {
            if name == "search_knowledge" {
                *search_calls += 1;
            }
            let result = exec_tool(client, name, args).await;
            push_message(
                messages,
                json!({ "role": "tool", "tool_call_id": id, "content": result }),
            );
        }
    }
    Err(format!("no text answer within {MAX_ROUNDS} tool round-trips"))
}

/// A single (model, condition) run: initial generation + up to `repair_rounds` repairs on failure.
/// Feeds the failure detail (syntax stderr, runtime error, behavior shortfall) back to have it fixed in the same conversation.
async fn run_with_repair(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    condition: &'static str,
    bridge: Option<(&RunningService<RoleClient, ()>, &Value)>,
    repair_rounds: usize,
    tmp: &std::path::Path,
) -> CodeResult {
    let initial_prompt = if bridge.is_some() {
        format!("{TASK}{DELEGATED_SUFFIX}")
    } else {
        TASK.to_string()
    };
    let mut messages = json!([{ "role": "user", "content": initial_prompt }]);
    let mut search_calls = 0usize;
    let mut tokens_sum = 0u64;
    let mut rounds: Vec<String> = Vec::new();
    let mut last_eval: Option<Evaluation> = None;
    let mut error: Option<String> = None;

    for r in 0..=repair_rounds {
        match next_answer(http, base, model, &mut messages, bridge, &mut search_calls, &mut tokens_sum)
            .await
        {
            Err(e) => {
                // Transport/protocol errors cannot be fixed by feedback - stop.
                rounds.push(format!("r{r}: error - {e}"));
                error = Some(e);
                break;
            }
            Ok(text) => {
                let eval = evaluate(&text, tmp);
                let label = eval.label();
                eprintln!("    [r{r}] {label}");
                rounds.push(format!("r{r}: {label}"));
                let done = eval.success() || r == repair_rounds;
                if !done {
                    push_message(
                        &mut messages,
                        json!({ "role": "user", "content": eval.feedback() }),
                    );
                }
                let stop = eval.success();
                last_eval = Some(eval);
                if stop {
                    break;
                }
            }
        }
    }

    let initial_success = rounds
        .first()
        .is_some_and(|r0| r0.contains("success"));
    match last_eval {
        Some(eval) => {
            let success = eval.success();
            CodeResult {
                model: model.into(),
                condition,
                syntax_ok: eval.syntax.as_ref().map(|(ok, _)| *ok),
                html: eval.html,
                fingerprints: eval.fingerprints,
                behavior: eval.behavior,
                search_calls,
                tokens: tokens_sum,
                error,
                rounds,
                initial_success,
                success,
            }
        }
        None => CodeResult {
            model: model.into(),
            condition,
            html: None,
            fingerprints: FINGERPRINTS.iter().map(|(n, _)| (*n, false)).collect(),
            syntax_ok: None,
            behavior: None,
            search_calls,
            tokens: tokens_sum,
            error,
            rounds,
            initial_success: false,
            success: false,
        },
    }
}

/// Loads the shared ontology into the engine.
fn load_ontology(engine: &Engine) {
    for d in design_decisions() {
        engine
            .observe(ObserveInput {
                content: d.content.into(),
                workspace: Some(WS.into()),
                source_ref: Some("design/physics-demo.md".into()),
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: d
                    .entities
                    .iter()
                    .map(|(n, t)| EntityInput { name: (*n).into(), kind: Some((*t).into()) })
                    .collect(),
                relations: d
                    .relations
                    .iter()
                    .map(|(f, k, t)| RelationInput {
                        from: (*f).into(),
                        kind: (*k).into(),
                        to: (*t).into(),
                        valid_from: None,
                        valid_to: None,
                    })
                    .collect(),
            })
            .expect("load design decision");
    }
}

// --- Gallery/report ----------------------------------------------------------

fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

/// A self-contained gallery placing all demos side by side in an iframe(srcdoc) grid.
fn render_gallery(results: &[CodeResult]) -> String {
    let mut cells = String::new();
    for r in results {
        let badge = match (&r.html, r.syntax_ok) {
            (None, _) => "format violation - no html code block".to_string(),
            (_, Some(false)) => "js syntax error".to_string(),
            _ => {
                let behavior = r
                    .behavior
                    .as_ref()
                    .map(|b| b.label())
                    .unwrap_or_else(|| "behavior not measured".into());
                format!(
                    "{} / design conformance {}/{} / {} searches / {behavior}",
                    r.rounds_label(),
                    r.fp_hits(),
                    r.fingerprints.len(),
                    r.search_calls
                )
            }
        };
        let frame = match &r.html {
            Some(h) => format!(
                "<div class=\"frame\"><iframe sandbox=\"allow-scripts\" srcdoc=\"{}\"></iframe></div>",
                attr_escape(h)
            ),
            None => {
                let why = r.error.as_deref().unwrap_or("could not find html in the response");
                format!("<div class=\"empty\">{}</div>", attr_escape(why))
            }
        };
        cells.push_str(&format!(
            "<figure><figcaption><b>{}</b> / {} <span>{}</span></figcaption>{}</figure>\n",
            r.model, r.condition, badge, frame
        ));
    }
    format!(
        r##"<!doctype html>
<meta charset="utf-8">
<title>physics coding eval - demo gallery</title>
<style>
  :root {{ color-scheme: dark; }}
  body {{ margin: 0; background: #101216; color: #d8dee9; font: 14px/1.5 system-ui, sans-serif; padding: 16px; }}
  h1 {{ font-size: 16px; margin: 0 0 4px; }}
  p.sub {{ margin: 0 0 16px; color: #9aa5b1; font-size: 12.5px; }}
  .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(430px, 1fr)); gap: 16px; }}
  figure {{ margin: 0; background: #171a20; border: 1px solid #2a2f38; border-radius: 8px; padding: 10px; }}
  figcaption {{ margin-bottom: 8px; font-size: 13px; }}
  figcaption span {{ color: #9aa5b1; margin-left: 8px; font-size: 12px; }}
  /* So the demo canvas (500-600px tall) is not clipped: fit a 2x virtual viewport scaled down to 0.5. */
  .frame {{ width: 100%; aspect-ratio: 3 / 2; overflow: hidden; position: relative;
           border-radius: 6px; background: #14161a; }}
  .frame iframe {{ width: 200%; height: 200%; transform: scale(0.5); transform-origin: 0 0;
                  border: 0; position: absolute; left: 0; top: 0; }}
  .empty {{ aspect-ratio: 3 / 2; display: grid; place-items: center; color: #7f6a6a;
           font-size: 12.5px; border: 1px dashed #3a2f2f; border-radius: 6px; padding: 12px;
           text-align: center; }}
</style>
<h1>physics coding eval - demo gallery</h1>
<p class="sub">bare = task only / delegated = query the shared ontology over MCP. "design conformance n/7" is
the number of design fingerprints that exist only in the knowledge base (gravity 900, restitution 0.8, damping 0.999, dt 1/60,
impulse (1+e), invMass, positional correction) appearing in the code.</p>
<div class="grid">
{cells}</div>
"##
    )
}

fn render_markdown(results: &[CodeResult]) -> String {
    let mut md = String::new();
    md.push_str("# physics coding eval report\n\n");
    md.push_str("Whether a shared-ontology (team design decisions) delegation shows up in coding output.\n\n");
    let initial_ok = results.iter().filter(|r| r.initial_success).count();
    let final_ok = results.iter().filter(|r| r.success).count();
    md.push_str(&format!(
        "**success rate: initial generation {initial_ok}/{} -> after repair {final_ok}/{}** \
         (success = syntax + runs + moves + bounces + contained, all pass)\n\n",
        results.len(),
        results.len()
    ));
    md.push_str("| model | condition | rounds | js syntax | design conformance | behavior | searches | tokens |\n");
    md.push_str("|---|---|---|---|---|---|---|---|\n");
    for r in results {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {} | {} | {} |\n",
            r.model,
            r.condition,
            r.rounds_label(),
            match r.syntax_ok {
                Some(true) => "ok",
                Some(false) => "error",
                None => "-",
            },
            r.fp_hits(),
            r.fingerprints.len(),
            r.behavior.as_ref().map(|b| b.label()).unwrap_or_else(|| "-".into()),
            r.search_calls,
            r.tokens
        ));
    }
    md.push_str("\n## Prompts used (identical for all models)\n\n");
    md.push_str("Task (bare receives only this):\n\n```text\n");
    md.push_str(TASK);
    md.push_str("\n```\n\ndelegated additional instruction:\n\n```text\n");
    md.push_str(DELEGATED_SUFFIX.trim_start());
    md.push_str("\n```\n\nRepair-round feedback template ({issues} = the automatic scorer's failure items):\n\n```text\n");
    md.push_str(FEEDBACK_TEMPLATE);
    md.push_str("\n```\n");

    md.push_str("\n## Round log (the repair convergence process)\n\n");
    for r in results {
        md.push_str(&format!(
            "- {} / {}: {}\n",
            r.model,
            r.condition,
            r.rounds.join(" -> ")
        ));
    }
    md.push_str("\n## Fingerprint detail\n\n");
    for r in results {
        let hits: Vec<String> = r
            .fingerprints
            .iter()
            .map(|(n, h)| format!("{}{}", if *h { "[o] " } else { "[x] " }, n))
            .collect();
        md.push_str(&format!("- {} / {}: {}\n", r.model, r.condition, hits.join(", ")));
        if let Some(e) = &r.error {
            md.push_str(&format!("  - error: {e}\n"));
        }
    }
    md
}

// --- Main --------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires local Ollama - manual eval of ontology-delegation coding output"]
async fn delegated_ontology_improves_coding() {
    let base = std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string());
    let models_env = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| DEFAULT_MODELS.to_string());
    let models: Vec<&str> = models_env
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let dir = report_dir();
    let tmp = dir.join("physics_demos");
    std::fs::create_dir_all(&tmp).expect("create demo directory");
    let mut results: Vec<CodeResult> = Vec::new();

    // EVAL_REPLAY=1: re-score the saved demo files without model calls (fingerprints + syntax + behavior).
    // Used to deterministically re-score exactly the artifacts a human saw in the gallery.
    if std::env::var("EVAL_REPLAY").as_deref() == Ok("1") {
        let mut names: Vec<_> = std::fs::read_dir(&tmp)
            .expect("read physics_demos")
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".html"))
            .collect();
        names.sort();
        for name in names {
            let (model, condition) = match name.strip_suffix("_delegated.html") {
                Some(m) => (m.to_string(), "delegated"),
                None => match name.strip_suffix("_bare.html") {
                    Some(m) => (m.to_string(), "bare"),
                    None => continue,
                },
            };
            let html = std::fs::read_to_string(tmp.join(&name)).expect("read demo");
            // The saved copy is already pure html, so wrap it in a code block to run the same evaluation path.
            let eval = evaluate(&format!("```html\n{html}\n```"), &tmp);
            let success = eval.success();
            let r = CodeResult {
                model: model.clone(),
                condition,
                syntax_ok: eval.syntax.as_ref().map(|(ok, _)| *ok),
                html: eval.html,
                fingerprints: eval.fingerprints,
                behavior: eval.behavior,
                search_calls: 0,
                tokens: 0,
                error: None,
                rounds: vec![format!("replay: {}", if success { "success" } else { "failed" })],
                initial_success: success,
                success,
            };
            eprintln!(
                "  [replay] {model:<16} {condition:<10} design conformance {}/{}  {}",
                r.fp_hits(),
                r.fingerprints.len(),
                r.behavior.as_ref().map(|b| b.label()).unwrap_or_else(|| "-".into())
            );
            results.push(r);
        }
        report::write_report("physics_gallery.html", &render_gallery(&results));
        report::write_report("physics_coding_eval.md", &render_markdown(&results));
        eprintln!("\n[gallery] {}", dir.join("physics_gallery.html").display());
        assert!(!results.is_empty(), "no demo files to re-score");
        return;
    }

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("http client");

    if !ollama_reachable(&http, &base).await {
        eprintln!("[skip] cannot reach Ollama ({base}) - rerun after `ollama serve`");
        return;
    }

    let repair_rounds: usize = std::env::var("EVAL_REPAIR_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    eprintln!("[config] up to {repair_rounds} repair rounds (EVAL_REPAIR_ROUNDS)");

    for model in &models {
        eprintln!("\n=== model: {model} ===");

        // MCP server carrying the shared ontology (fresh per model - state isolation).
        let engine = Arc::new(
            Engine::new(Arc::new(InMemoryStore::new()), "physics-eval", WS)
                .with_embedder(Arc::new(HashingEmbedder::default())),
        );
        load_ontology(&engine);
        let (client, server) = serve_engine(engine).await;
        let tools = openai_tools(&client).await;

        for condition in ["bare", "delegated"] {
            eprintln!("  [{condition}]");
            let bridge = match condition {
                "bare" => None,
                _ => Some((&client, &tools)),
            };
            let r = run_with_repair(&http, &base, model, condition, bridge, repair_rounds, &tmp)
                .await;
            eprintln!(
                "  [{condition:<9}] {}  design conformance {}/{}  {} searches  ({}tk)",
                r.rounds_label(),
                r.fp_hits(),
                r.fingerprints.len(),
                r.search_calls,
                r.tokens
            );
            if let Some(h) = &r.html {
                let safe_model = model.replace(['/', ':'], "_");
                let path = tmp.join(format!("{safe_model}_{condition}.html"));
                std::fs::write(&path, h).expect("save demo");
            }
            results.push(r);
        }

        let _ = client.cancel().await;
        let _ = server.await;
    }

    eprintln!("\n=== comparison (design conformance = number of fingerprints where knowledge flowed into code) ===");
    for r in &results {
        eprintln!(
            "  {:<14} {:<10} design conformance {}/{}  {} searches",
            r.model,
            r.condition,
            r.fp_hits(),
            r.fingerprints.len(),
            r.search_calls
        );
    }

    let gallery_path = report::write_report("physics_gallery.html", &render_gallery(&results));
    let md_path = report::write_report("physics_coding_eval.md", &render_markdown(&results));
    eprintln!("\n[gallery] {}", gallery_path.display());
    eprintln!("[report]  {}", md_path.display());

    // Harness sanity guard: if no (model, condition) produces html, the bridge/extraction is broken.
    assert!(
        results.iter().any(|r| r.html.is_some()),
        "no model produced an html demo - check the bridge/extraction"
    );
}
