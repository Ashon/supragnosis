//! Ontology build eval: scores whether small models "build" an ontology well, as a by-product of work.
//!
//! Where delegation_eval measured recall (read) quality, this harness measures ingestion (write)
//! quality. Principle 22 (knowledge management is a by-product of work): give the model the design
//! notes of a 2D physics engine in chunks, and have it build the ontology itself by extracting
//! entities/relations via observe as it reads each chunk.
//!
//! Quantitative metrics (per model):
//!   - observe call rate: how many of the 10 chunks it actually ingested.
//!   - node/edge/relation-type counts: the volume and structure of the graph.
//!   - core-concept coverage: whether the 11 gold concepts of the physics-engine domain exist as nodes.
//!   - isolated nodes: the count of entities thrown in with no relation (connection failure).
//!   - suspected-duplicate pairs: node pairs where one name contains the other (a normalization-failure signal).
//!
//! Qualitative artifacts (whose purpose is human inspection):
//!   - target/eval-reports/ontology_viewer.html - a self-contained force-layout viewer (no external
//!     dependencies) that switches between per-model ontology graphs via tabs.
//!   - target/eval-reports/ontology_build_eval.md - per-chunk extraction log + metrics.
//!
//! The graph is also read once via the real MCP resource (supragnosis://workspace/{ws}/graph) to
//! validate the surface, while the viewer data uses the full engine.graph(None) projection - so
//! that even the mistake of a model making up a workspace argument and leaking into another
//! workspace shows up in the graph.
//!
//! Run:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test ontology_build_eval -- --ignored --nocapture
//! Optional env: OLLAMA_BASE_URL (default http://localhost:11434), OLLAMA_MODELS (default gemma4),
//!   EVAL_SCHEMA_HINT (default 1) - illustrates in the prompt the exact JSON shape of the observe
//!   arguments. 0 gives only the tool schema and drops the prompt hint - a friction-measurement
//!   mode for whether small models match the field names from the schema alone (surface legibility,
//!   Principle 21). First measurement: without the hint, gemma4 made up field names like
//!   from_entity_id/relation_type and was wiped out 0/10 (the extraction itself was sophisticated).
//!
//! If Ollama is not up, it silently passes (skips).

use std::sync::Arc;
use std::time::Duration;

use rmcp::model::{ReadResourceRequestParams, ResourceContents};
use serde_json::{json, Map, Value};

use supragnosis_e2e::bridge::{
    chat, exec_tool, ollama_reachable, openai_tools, serve_engine, tool_calls, DEFAULT_BASE,
};
use supragnosis_e2e::report;
use supragnosis_engine::Engine;
use supragnosis_store::InMemoryStore;

const DEFAULT_MODELS: &str = "gemma4";
const WS: &str = "physics";

/// Physics-engine design notes. The model must extract entities/relations via observe for each chunk.
const NOTES: [&str; 10] = [
    "the engine represents positions, velocities and forces as 2d vectors; vector addition, \
     scaling and dot product are the core operations",
    "a rigid body stores mass, position, velocity and an accumulated force; a static body has \
     infinite mass and never moves",
    "the integrator advances every rigid body with semi-implicit euler: it updates velocity \
     from acceleration first, then position from the new velocity",
    "gravity is a global force generator: each step it applies a constant downward force \
     proportional to mass to every dynamic body",
    "collision detection runs in two phases; the broadphase prunes candidate pairs cheaply \
     using axis aligned bounding boxes (aabb)",
    "the narrowphase tests each candidate pair exactly: a circle versus circle test produces \
     a contact with a normal and a penetration depth",
    "collision response applies an impulse along the contact normal, scaled by the restitution \
     of the two bodies, to push them apart",
    "positional correction moves overlapping bodies apart in proportion to penetration depth \
     so stacked bodies do not sink into each other",
    "the world owns all bodies and runs the step loop: apply forces, integrate, detect \
     collisions, resolve contacts, correct positions",
    "linear damping multiplies each velocity by a factor slightly below one every step, \
     approximating drag and keeping the simulation stable",
];

/// Gold-concept coverage: (name, substrings to look for in node names, any-of).
const GOLD_CONCEPTS: [(&str, &[&str]); 11] = [
    ("vector", &["vector"]),
    ("rigid body", &["rigid"]),
    ("integrator", &["integrat", "euler"]),
    ("gravity", &["gravity"]),
    ("broadphase", &["broadphase", "broad phase", "aabb", "bounding box"]),
    ("narrowphase", &["narrowphase", "narrow phase", "circle"]),
    ("contact", &["contact", "penetration"]),
    ("impulse", &["impulse", "restitution"]),
    ("correction", &["correction"]),
    ("world/step", &["world", "step loop", "step"]),
    ("damping", &["damping", "drag"]),
];

// --- Ollama bridge (same pattern as delegation_eval.rs) ----------------------

// --- Scoring -----------------------------------------------------------------

/// Ingestion record for a single chunk.
struct ChunkLog {
    note_idx: usize,
    /// Whether observe was called. If it was not called, or execution errored, holds the reason.
    outcome: Result<(usize, usize), String>, // (entities, relations)
    args_summary: String,
}

/// Ontology build result for a single model.
struct BuildResult {
    model: String,
    chunks: Vec<ChunkLog>,
    /// Full engine.graph(None) projection JSON (viewer data).
    graph: Value,
    /// Whether the default-ws graph read via the MCP resource surface was valid JSON.
    resource_ok: bool,
}

impl BuildResult {
    fn observed(&self) -> usize {
        self.chunks.iter().filter(|c| c.outcome.is_ok()).count()
    }

    fn node_names(&self) -> Vec<String> {
        self.graph["nodes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|n| n["name"].as_str().map(|s| s.to_lowercase()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn coverage(&self) -> Vec<(&'static str, bool)> {
        let names = self.node_names();
        GOLD_CONCEPTS
            .iter()
            .map(|(label, pats)| {
                let hit = names
                    .iter()
                    .any(|n| pats.iter().any(|p| n.contains(p)));
                (*label, hit)
            })
            .collect()
    }

    fn isolated_nodes(&self) -> usize {
        self.graph["nodes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|n| n["degree"].as_u64() == Some(0))
                    .count()
            })
            .unwrap_or(0)
    }

    /// Suspected-duplicate pairs: one node name contains another (both >= 4 chars, different ids).
    fn dup_pairs(&self) -> usize {
        let names = self.node_names();
        let mut count = 0;
        for (i, a) in names.iter().enumerate() {
            for b in names.iter().skip(i + 1) {
                if a.len() >= 4 && b.len() >= 4 && a != b && (a.contains(b) || b.contains(a)) {
                    count += 1;
                }
            }
        }
        count
    }

    fn stats(&self) -> (u64, u64, usize) {
        let nodes = self.graph["stats"]["node_count"].as_u64().unwrap_or(0);
        let edges = self.graph["stats"]["edge_count"].as_u64().unwrap_or(0);
        let kinds: std::collections::HashSet<&str> = self.graph["edges"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| e["type"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        (nodes, edges, kinds.len())
    }
}

// --- Execution ---------------------------------------------------------------

/// Chunk prompt template. {i}/{n} = chunk number, {note} = design note, {hint} = schema hint.
/// Execution and the report use the same source text.
const CHUNK_PROMPT: &str = "You are implementing a simple 2D physics engine. Knowledge \
management is a by-product of your work: as you read each design note, you record it in the \
knowledge base.\n\nDesign note {i}/{n}:\n{note}\n\nCall the observe tool now with:\n\
- content: the note text\n\
- entities: the key concepts, components, algorithms or quantities the note mentions (each \
with a name and a type such as Concept, Component, Algorithm, Quantity)\n\
- relations: typed links between those entities that the note supports (for example part_of, \
uses, computes, produces, applies_to)\n\
Extract only what this note supports.{hint}";

/// Schema hint (EVAL_SCHEMA_HINT=1, default). The header comment records the measurement where,
/// without the hint, small models made up field names and were wiped out.
const SCHEMA_HINT: &str = "\n\nThe observe tool expects exactly this argument shape (field \
names matter):\n{\"content\": \"...\", \"entities\": [{\"name\": \"rigid body\", \
\"type\": \"Concept\"}], \"relations\": [{\"from\": \"integrator\", \"type\": \
\"uses\", \"to\": \"rigid body\"}]}\nUse exactly the field names name, type, from, to. \
relations must reference entity names from the entities list.";

/// Feeds the entire set of notes to a single model in chunks and has it build the ontology.
async fn build_ontology(
    http: &reqwest::Client,
    base: &str,
    model: &str,
) -> BuildResult {
    // Start from an empty engine - the ontology is built entirely from the model's extraction.
    let engine = Arc::new(Engine::new(
        Arc::new(InMemoryStore::new()),
        "ontology-eval",
        WS,
    ));
    let engine_view = engine.clone();
    let (client, server) = serve_engine(engine).await;
    let tools = openai_tools(&client).await;

    let schema_hint = std::env::var("EVAL_SCHEMA_HINT").as_deref() != Ok("0");
    let mut chunks = Vec::new();
    for (i, note) in NOTES.iter().enumerate() {
        let hint = if schema_hint { SCHEMA_HINT } else { "" };
        let prompt = CHUNK_PROMPT
            .replace("{i}", &(i + 1).to_string())
            .replace("{n}", &NOTES.len().to_string())
            .replace("{note}", note)
            .replace("{hint}", hint);
        let messages = json!([{ "role": "user", "content": prompt }]);
        let outcome = match chat(http, base, model, &messages, Some(&tools)).await {
            Err(e) => ChunkLog {
                note_idx: i,
                outcome: Err(format!("chat error: {e}")),
                args_summary: String::new(),
            },
            Ok((msg, _)) => {
                let calls = tool_calls(&msg);
                match calls.iter().find(|(_, n, _)| n == "observe") {
                    None => ChunkLog {
                        note_idx: i,
                        outcome: Err("observe not called".into()),
                        args_summary: String::new(),
                    },
                    Some((_, _, args)) => {
                        let result = exec_tool(&client, "observe", args).await;
                        let ents = args["entities"].as_array().map(Vec::len).unwrap_or(0);
                        let rels = args["relations"].as_array().map(Vec::len).unwrap_or(0);
                        let summary = serde_json::to_string(args).unwrap_or_default();
                        if result.contains("\"error\"") {
                            ChunkLog {
                                note_idx: i,
                                outcome: Err(format!("observe execution failed: {result}")),
                                args_summary: summary,
                            }
                        } else {
                            ChunkLog {
                                note_idx: i,
                                outcome: Ok((ents, rels)),
                                args_summary: summary,
                            }
                        }
                    }
                }
            }
        };
        let label = match &outcome.outcome {
            Ok((e, r)) => format!("ok (entities {e}, relations {r})"),
            Err(why) => format!("miss - {why}"),
        };
        eprintln!("  [chunk {}] {label}", i + 1);
        chunks.push(outcome);
    }

    // Surface validation: read the default-ws graph via the MCP resource.
    let resource_ok = match client
        .read_resource(ReadResourceRequestParams {
            meta: None,
            uri: format!("supragnosis://workspace/{WS}/graph"),
        })
        .await
    {
        Ok(read) => read
            .contents
            .first()
            .and_then(|c| match c {
                ResourceContents::TextResourceContents { text, .. } => {
                    serde_json::from_str::<Value>(text).ok()
                }
                _ => None,
            })
            .is_some(),
        Err(_) => false,
    };

    // Viewer data: the full projection (workspace-unrestricted) - even a model making up a ws and leaking shows up.
    let graph = serde_json::to_value(engine_view.graph(None).expect("graph projection"))
        .expect("graph serialize");

    let _ = client.cancel().await;
    let _ = server.await;

    BuildResult {
        model: model.to_string(),
        chunks,
        graph,
        resource_ok,
    }
}

// --- Report/viewer -----------------------------------------------------------

fn render_markdown(results: &[BuildResult]) -> String {
    let mut md = String::new();
    md.push_str("# ontology build eval report\n\n");
    md.push_str("Scoring of the ontology a model builds via observe as it reads 10 chunks of physics-engine design notes.\n\n");
    md.push_str("## Quantitative summary\n\n");
    md.push_str("| model | observe calls | nodes | edges | relation types | coverage | isolated nodes | suspected-duplicate pairs | resource surface |\n");
    md.push_str("|---|---|---|---|---|---|---|---|---|\n");
    for r in results {
        let (nodes, edges, kinds) = r.stats();
        let cov = r.coverage();
        let cov_hit = cov.iter().filter(|(_, h)| *h).count();
        md.push_str(&format!(
            "| {} | {}/{} | {} | {} | {} | {}/{} | {} | {} | {} |\n",
            r.model,
            r.observed(),
            NOTES.len(),
            nodes,
            edges,
            kinds,
            cov_hit,
            cov.len(),
            r.isolated_nodes(),
            r.dup_pairs(),
            if r.resource_ok { "ok" } else { "fail" }
        ));
    }
    md.push_str("\n## Coverage detail\n\n");
    for r in results {
        let miss: Vec<&str> = r
            .coverage()
            .into_iter()
            .filter(|(_, h)| !h)
            .map(|(l, _)| l)
            .collect();
        md.push_str(&format!(
            "- {}: missing concepts = {}\n",
            r.model,
            if miss.is_empty() { "none".to_string() } else { miss.join(", ") }
        ));
    }
    md.push_str("\n## Prompts used (identical for all models)\n\n");
    md.push_str("Chunk template ({i}/{n} = chunk number, {note} = design note, {hint} = the hint below):\n\n```text\n");
    md.push_str(CHUNK_PROMPT);
    md.push_str("\n```\n\nSchema hint (EVAL_SCHEMA_HINT=1 default, omitted if 0):\n\n```text\n");
    md.push_str(SCHEMA_HINT);
    md.push_str("\n```\n\nDesign note source text:\n\n");
    for (i, note) in NOTES.iter().enumerate() {
        md.push_str(&format!("{}. {}\n", i + 1, note));
    }

    md.push_str("\n## Per-chunk ingestion log\n");
    for r in results {
        md.push_str(&format!("\n### {}\n\n", r.model));
        for c in &r.chunks {
            match &c.outcome {
                Ok((e, r_)) => md.push_str(&format!(
                    "- chunk {}: ok - entities {e}, relations {r_}\n  - args: `{}`\n",
                    c.note_idx + 1,
                    c.args_summary.replace('`', "'")
                )),
                Err(why) => {
                    md.push_str(&format!(
                        "- chunk {}: miss - {}\n",
                        c.note_idx + 1,
                        why.replace('`', "'")
                    ));
                    // Even on failure, keep the arguments the model sent - qualitative evidence of surface friction.
                    if !c.args_summary.is_empty() {
                        md.push_str(&format!(
                            "  - args: `{}`\n",
                            c.args_summary.replace('`', "'")
                        ));
                    }
                }
            }
        }
    }
    md
}

/// Self-contained HTML viewer. Draws the graph with a canvas force layout, no external dependencies.
/// (Draws the physics ontology as a mini physics simulation.)
const VIEWER_TEMPLATE: &str = r##"<!doctype html>
<meta charset="utf-8">
<title>supragnosis ontology viewer</title>
<style>
  :root { color-scheme: dark; }
  body { margin: 0; background: #14161a; color: #d8dee9; font: 14px/1.5 system-ui, sans-serif; }
  header { padding: 10px 16px; display: flex; gap: 12px; align-items: center; flex-wrap: wrap; }
  h1 { font-size: 15px; margin: 0 12px 0 0; font-weight: 600; }
  .tab { padding: 5px 12px; border-radius: 6px; background: #22262d; cursor: pointer; border: 1px solid #333a44; }
  .tab.on { background: #3b4d68; border-color: #5b7ea8; }
  #stats { padding: 0 16px 8px; color: #9aa5b1; font-size: 12.5px; }
  #legend { padding: 0 16px 10px; font-size: 12px; color: #9aa5b1; display: flex; gap: 14px; flex-wrap: wrap; }
  .sw { display: inline-block; width: 10px; height: 10px; border-radius: 5px; margin-right: 5px; vertical-align: -1px; }
  canvas { display: block; }
  #tip { position: fixed; pointer-events: none; background: #0d0f12ee; border: 1px solid #3b4d68;
         padding: 6px 9px; border-radius: 6px; font-size: 12.5px; display: none; max-width: 320px; }
</style>
<header><h1>ontology viewer - physics-engine ingestion result</h1><div id="tabs"></div></header>
<div id="stats"></div>
<div id="legend"></div>
<canvas id="c"></canvas>
<div id="tip"></div>
<script>
const GRAPHS = __DATA__;
const PALETTE = ["#7aa2f7","#9ece6a","#e0af68","#f7768e","#bb9af7","#7dcfff","#ff9e64","#73daca","#c0caf5","#e46876"];
const canvas = document.getElementById("c"), ctx = canvas.getContext("2d");
const tip = document.getElementById("tip");
let nodes = [], edges = [], typeColor = {}, drag = null, hover = null, current = null;

function pick(name) {
  current = name;
  document.querySelectorAll(".tab").forEach(t => t.classList.toggle("on", t.textContent === name));
  const g = GRAPHS[name];
  // Node initial positions are a deterministic, index-based layout (golden-angle spiral).
  nodes = g.nodes.map((n, i) => {
    const a = i * 2.39996, r = 40 + 14 * Math.sqrt(i);
    return { ...n, x: innerWidth/2 + r * Math.cos(a), y: innerHeight/2 + r * Math.sin(a), vx: 0, vy: 0 };
  });
  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  edges = g.edges.map(e => ({ ...e, a: byId[e.from], b: byId[e.to] })).filter(e => e.a && e.b);
  typeColor = {};
  let ti = 0;
  for (const n of nodes) if (!(n.type in typeColor)) typeColor[n.type] = PALETTE[ti++ % PALETTE.length];
  const kinds = {};
  for (const e of edges) kinds[e.type] = (kinds[e.type] || 0) + 1;
  document.getElementById("stats").textContent =
    `nodes ${g.stats.node_count} / edges ${g.stats.edge_count}` +
    ` / types ${Object.keys(g.stats.type_counts).map(t => t + " " + g.stats.type_counts[t]).join(", ")}`;
  document.getElementById("legend").innerHTML =
    Object.entries(typeColor).map(([t, c]) => `<span><span class="sw" style="background:${c}"></span>${t}</span>`).join("") +
    `<span style="margin-left:auto">relations: ${Object.entries(kinds).map(([k, v]) => k + " x" + v).join(", ") || "none"}</span>`;
}

function resize() {
  canvas.width = innerWidth;
  canvas.height = innerHeight - canvas.getBoundingClientRect().top;
}
addEventListener("resize", resize);

function stepSim() {
  // Repulsion + spring + center attraction. At a scale of a few dozen nodes, O(n^2) is enough.
  for (let i = 0; i < nodes.length; i++) for (let j = i + 1; j < nodes.length; j++) {
    const a = nodes[i], b = nodes[j];
    let dx = b.x - a.x, dy = b.y - a.y, d2 = dx*dx + dy*dy || 1, d = Math.sqrt(d2);
    const f = 2600 / d2;
    dx /= d; dy /= d;
    a.vx -= f * dx; a.vy -= f * dy; b.vx += f * dx; b.vy += f * dy;
  }
  for (const e of edges) {
    let dx = e.b.x - e.a.x, dy = e.b.y - e.a.y, d = Math.hypot(dx, dy) || 1;
    const f = (d - 110) * 0.02;
    dx /= d; dy /= d;
    e.a.vx += f * dx; e.a.vy += f * dy; e.b.vx -= f * dx; e.b.vy -= f * dy;
  }
  const cx = canvas.width / 2, cy = canvas.height / 2;
  for (const n of nodes) {
    n.vx += (cx - n.x) * 0.002; n.vy += (cy - n.y) * 0.002;
    if (n !== drag) { n.x += n.vx *= 0.85; n.y += n.vy *= 0.85; }
  }
}

function draw() {
  stepSim();
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  for (const e of edges) {
    ctx.beginPath();
    ctx.strokeStyle = e.valid_to ? "#4a3f4f" : "#39414d";
    ctx.setLineDash(e.valid_to ? [4, 4] : []);
    ctx.moveTo(e.a.x, e.a.y); ctx.lineTo(e.b.x, e.b.y); ctx.stroke();
    ctx.setLineDash([]);
    // Arrowhead.
    const dx = e.b.x - e.a.x, dy = e.b.y - e.a.y, d = Math.hypot(dx, dy) || 1;
    const tx = e.b.x - dx / d * 14, ty = e.b.y - dy / d * 14;
    ctx.beginPath();
    ctx.moveTo(tx, ty);
    ctx.lineTo(tx - dy / d * 3 - dx / d * 5, ty + dx / d * 3 - dy / d * 5);
    ctx.lineTo(tx + dy / d * 3 - dx / d * 5, ty - dx / d * 3 - dy / d * 5);
    ctx.fillStyle = "#4c5666"; ctx.fill();
  }
  for (const n of nodes) {
    const r = 6 + Math.min(8, n.degree * 1.5);
    ctx.beginPath();
    ctx.fillStyle = typeColor[n.type] || "#888";
    ctx.arc(n.x, n.y, r, 0, 7); ctx.fill();
    if (n === hover) { ctx.strokeStyle = "#fff"; ctx.stroke(); }
    ctx.fillStyle = "#c3cad4"; ctx.font = "11px system-ui";
    ctx.fillText(n.name, n.x + r + 4, n.y + 4);
  }
  requestAnimationFrame(draw);
}

function nodeAt(x, y) {
  return nodes.find(n => Math.hypot(n.x - x, n.y - y) < 14) || null;
}
canvas.addEventListener("mousemove", ev => {
  const n = nodeAt(ev.offsetX, ev.offsetY);
  hover = n;
  if (drag) { drag.x = ev.offsetX; drag.y = ev.offsetY; }
  if (n) {
    tip.style.display = "block";
    tip.style.left = ev.clientX + 14 + "px";
    tip.style.top = ev.clientY + 14 + "px";
    tip.innerHTML = `<b>${n.name}</b><br>type ${n.type} / degree ${n.degree}` +
      `<br>sources ${n.sources} / trust ${n.trust_tier}`;
  } else tip.style.display = "none";
});
canvas.addEventListener("mousedown", ev => { drag = nodeAt(ev.offsetX, ev.offsetY); });
addEventListener("mouseup", () => { drag = null; });

const tabs = document.getElementById("tabs");
for (const name of Object.keys(GRAPHS)) {
  const b = document.createElement("div");
  b.className = "tab"; b.textContent = name;
  b.onclick = () => pick(name);
  tabs.appendChild(b);
}
resize();
pick(Object.keys(GRAPHS)[0]);
draw();
</script>
"##;

fn write_outputs(results: &[BuildResult]) -> (std::path::PathBuf, std::path::PathBuf) {
    let md_path = report::write_report("ontology_build_eval.md", &render_markdown(results));

    let data: Map<String, Value> = results
        .iter()
        .map(|r| (r.model.clone(), r.graph.clone()))
        .collect();
    let html = VIEWER_TEMPLATE.replace(
        "__DATA__",
        &serde_json::to_string(&Value::Object(data)).expect("graph data json"),
    );
    let html_path = report::write_report("ontology_viewer.html", &html);

    (md_path, html_path)
}

// --- Main --------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires local Ollama - manual eval of ontology build (ingestion quality)"]
async fn small_models_build_physics_ontology() {
    let base = std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string());
    let models_env = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| DEFAULT_MODELS.to_string());
    let models: Vec<&str> = models_env
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("http client");

    if !ollama_reachable(&http, &base).await {
        eprintln!("[skip] cannot reach Ollama ({base}) - rerun after `ollama serve`");
        return;
    }

    let mut results = Vec::new();
    for model in &models {
        eprintln!("\n=== model: {model} ===");
        results.push(build_ontology(&http, &base, model).await);
    }

    eprintln!("\n=== comparison (observe / nodes / edges / relation types / coverage / isolated / duplicate pairs) ===");
    for r in &results {
        let (nodes, edges, kinds) = r.stats();
        let cov = r.coverage();
        let cov_hit = cov.iter().filter(|(_, h)| *h).count();
        eprintln!(
            "  {:<14} observe {}/{}  nodes {}  edges {}  kinds {}  coverage {}/{}  isolated {}  dup {}",
            r.model,
            r.observed(),
            NOTES.len(),
            nodes,
            edges,
            kinds,
            cov_hit,
            cov.len(),
            r.isolated_nodes(),
            r.dup_pairs()
        );
    }

    let (md_path, html_path) = write_outputs(&results);
    eprintln!("\n[report] {}", md_path.display());
    eprintln!("[viewer] {}", html_path.display());

    // Harness sanity guard: if no model can ingest even one chunk, the bridge/surface is broken.
    assert!(
        results.iter().any(|r| r.observed() > 0),
        "no model succeeded at observe even once - check the bridge/surface"
    );
}
