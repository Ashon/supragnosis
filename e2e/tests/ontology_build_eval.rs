//! Ontology build eval: 소형 모델이 작업 부산물로 온톨로지를 "잘 짓는가"를 채점한다.
//!
//! delegation_eval 이 회수(읽기) 품질을 쟀다면, 이 하네스는 적재(쓰기) 품질을 잰다.
//! 원칙 22(지식 관리는 작업의 부산물): 모델에게 2D 물리엔진 설계 노트를 청크로 주고,
//! 각 청크를 읽으며 observe 로 엔티티/관계를 추출해 온톨로지를 스스로 구축하게 한다.
//!
//! 정량 지표 (모델별):
//!   - observe 호출률: 청크 10개 중 몇 개를 실제로 적재했는가.
//!   - 노드/엣지/관계타입 수: 그래프의 부피와 구조.
//!   - 핵심 개념 커버리지: 물리엔진 도메인의 골드 개념 11종이 노드로 존재하는가.
//!   - 고립 노드: 관계 없이 엔티티만 던진 수(연결 실패).
//!   - 중복 의심 쌍: 한 이름이 다른 이름을 포함하는 노드 쌍(정규화 실패 신호).
//!
//! 정성 산출물 (사람 눈으로 검사하는 것이 목적):
//!   - target/eval-reports/ontology_viewer.html - 모델별 온톨로지 그래프를 탭으로
//!     전환하며 보는 자체 포함 포스 레이아웃 뷰어(외부 의존성 없음).
//!   - target/eval-reports/ontology_build_eval.md - 청크별 추출 로그 + 지표.
//!
//! 그래프는 실제 MCP 리소스(supragnosis://workspace/{ws}/graph)로도 한 번 읽어 표면을
//! 검증하고, 뷰어 데이터는 engine.graph(None) 전체 프로젝션을 쓴다 - 모델이 workspace
//! 인자를 지어내 다른 워크스페이스로 새는 실수까지 그래프에 드러나게 하기 위해서다.
//!
//! 실행:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test ontology_build_eval -- --ignored --nocapture
//! 선택 env: OLLAMA_BASE_URL (기본 http://localhost:11434), OLLAMA_MODELS (기본 gemma4),
//!   EVAL_SCHEMA_HINT (기본 1) - 프롬프트에 observe 인자의 정확한 JSON 형태를 예시한다.
//!   0 이면 도구 스키마만 주고 프롬프트 힌트를 뺀다 - 소형 모델이 스키마만으로 필드명을
//!   맞추는지(표면 가독성, 원칙 21)를 재는 마찰 측정 모드. 첫 실측: 힌트 없이는 gemma4 가
//!   from_entity_id/relation_type 등 필드명을 지어내 0/10 전멸했다(추출 자체는 정교했다).
//!
//! Ollama 가 안 떠 있으면 조용히 통과(skip)한다.

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

/// 물리엔진 설계 노트. 모델은 청크마다 observe 로 엔티티/관계를 추출해야 한다.
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

/// 골드 개념 커버리지: (이름, 노드명에서 찾을 부분문자열 any-of).
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

// --- Ollama 브리지 (delegation_eval.rs 와 같은 패턴) --------------------------

// --- 채점 --------------------------------------------------------------------

/// 한 청크의 적재 기록.
struct ChunkLog {
    note_idx: usize,
    /// observe 를 불렀는가. 부르지 않았거나 실행이 에러면 이유를 담는다.
    outcome: Result<(usize, usize), String>, // (entities, relations)
    args_summary: String,
}

/// 한 모델의 온톨로지 구축 결과.
struct BuildResult {
    model: String,
    chunks: Vec<ChunkLog>,
    /// engine.graph(None) 전체 프로젝션 JSON (뷰어 데이터).
    graph: Value,
    /// MCP 리소스 표면으로 읽은 기본 ws 그래프가 유효 JSON 이었는가.
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

    /// 중복 의심 쌍: 한 노드명이 다른 노드명을 포함(둘 다 4자 이상, 서로 다른 id).
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

// --- 실행 --------------------------------------------------------------------

/// 청크 프롬프트 템플릿. {i}/{n} = 청크 번호, {note} = 설계 노트, {hint} = 스키마 힌트.
/// 실행과 리포트가 같은 원문을 쓴다.
const CHUNK_PROMPT: &str = "You are implementing a simple 2D physics engine. Knowledge \
management is a by-product of your work: as you read each design note, you record it in the \
knowledge base.\n\nDesign note {i}/{n}:\n{note}\n\nCall the observe tool now with:\n\
- content: the note text\n\
- entities: the key concepts, components, algorithms or quantities the note mentions (each \
with a name and a type such as Concept, Component, Algorithm, Quantity)\n\
- relations: typed links between those entities that the note supports (for example part_of, \
uses, computes, produces, applies_to)\n\
Extract only what this note supports.{hint}";

/// 스키마 힌트(EVAL_SCHEMA_HINT=1, 기본). 힌트 없이는 소형 모델이 필드명을 지어내
/// 전멸했던 실측이 헤더 주석에 있다.
const SCHEMA_HINT: &str = "\n\nThe observe tool expects exactly this argument shape (field \
names matter):\n{\"content\": \"...\", \"entities\": [{\"name\": \"rigid body\", \
\"type\": \"Concept\"}], \"relations\": [{\"from\": \"integrator\", \"type\": \
\"uses\", \"to\": \"rigid body\"}]}\nUse exactly the field names name, type, from, to. \
relations must reference entity names from the entities list.";

/// 한 모델에게 노트 전체를 청크로 먹이며 온톨로지를 짓게 한다.
async fn build_ontology(
    http: &reqwest::Client,
    base: &str,
    model: &str,
) -> BuildResult {
    // 빈 엔진에서 시작한다 - 온톨로지는 전적으로 모델의 추출로 만들어진다.
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
                outcome: Err(format!("chat 오류: {e}")),
                args_summary: String::new(),
            },
            Ok((msg, _)) => {
                let calls = tool_calls(&msg);
                match calls.iter().find(|(_, n, _)| n == "observe") {
                    None => ChunkLog {
                        note_idx: i,
                        outcome: Err("observe 미호출".into()),
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
                                outcome: Err(format!("observe 실행 실패: {result}")),
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
            Ok((e, r)) => format!("ok (엔티티 {e}, 관계 {r})"),
            Err(why) => format!("miss - {why}"),
        };
        eprintln!("  [chunk {}] {label}", i + 1);
        chunks.push(outcome);
    }

    // 표면 검증: MCP 리소스로 기본 ws 그래프를 읽는다.
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

    // 뷰어 데이터: 전체 프로젝션(워크스페이스 무제한) - 모델이 ws 를 지어내 샌 것도 보인다.
    let graph = serde_json::to_value(engine_view.graph(None)).expect("graph serialize");

    let _ = client.cancel().await;
    let _ = server.await;

    BuildResult {
        model: model.to_string(),
        chunks,
        graph,
        resource_ok,
    }
}

// --- 리포트/뷰어 -------------------------------------------------------------

fn render_markdown(results: &[BuildResult]) -> String {
    let mut md = String::new();
    md.push_str("# ontology build eval 리포트\n\n");
    md.push_str("물리엔진 설계 노트 10청크를 읽으며 모델이 observe 로 구축한 온톨로지 채점.\n\n");
    md.push_str("## 정량 요약\n\n");
    md.push_str("| 모델 | observe 호출 | 노드 | 엣지 | 관계타입 | 커버리지 | 고립 노드 | 중복 의심 쌍 | 리소스 표면 |\n");
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
    md.push_str("\n## 커버리지 상세\n\n");
    for r in results {
        let miss: Vec<&str> = r
            .coverage()
            .into_iter()
            .filter(|(_, h)| !h)
            .map(|(l, _)| l)
            .collect();
        md.push_str(&format!(
            "- {}: 누락 개념 = {}\n",
            r.model,
            if miss.is_empty() { "없음".to_string() } else { miss.join(", ") }
        ));
    }
    md.push_str("\n## 사용 프롬프트 (전 모델 동일)\n\n");
    md.push_str("청크 템플릿 ({i}/{n} = 청크 번호, {note} = 설계 노트, {hint} = 아래 힌트):\n\n```text\n");
    md.push_str(CHUNK_PROMPT);
    md.push_str("\n```\n\n스키마 힌트 (EVAL_SCHEMA_HINT=1 기본, 0 이면 생략):\n\n```text\n");
    md.push_str(SCHEMA_HINT);
    md.push_str("\n```\n\n설계 노트 원문:\n\n");
    for (i, note) in NOTES.iter().enumerate() {
        md.push_str(&format!("{}. {}\n", i + 1, note));
    }

    md.push_str("\n## 청크별 적재 로그\n");
    for r in results {
        md.push_str(&format!("\n### {}\n\n", r.model));
        for c in &r.chunks {
            match &c.outcome {
                Ok((e, r_)) => md.push_str(&format!(
                    "- chunk {}: ok - 엔티티 {e}, 관계 {r_}\n  - args: `{}`\n",
                    c.note_idx + 1,
                    c.args_summary.replace('`', "'")
                )),
                Err(why) => {
                    md.push_str(&format!(
                        "- chunk {}: miss - {}\n",
                        c.note_idx + 1,
                        why.replace('`', "'")
                    ));
                    // 실패해도 모델이 보낸 인자를 남긴다 - 표면 마찰의 정성 증거.
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

/// 자체 포함 HTML 뷰어. 외부 의존성 없이 canvas 포스 레이아웃으로 그래프를 그린다.
/// (물리 온톨로지를 미니 물리 시뮬레이션으로 그린다.)
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
<header><h1>ontology viewer - 물리엔진 적재 결과</h1><div id="tabs"></div></header>
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
  // 노드 초기 위치는 인덱스 기반 결정적 배치(황금각 나선).
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
    `<span style="margin-left:auto">관계: ${Object.entries(kinds).map(([k, v]) => k + " x" + v).join(", ") || "없음"}</span>`;
}

function resize() {
  canvas.width = innerWidth;
  canvas.height = innerHeight - canvas.getBoundingClientRect().top;
}
addEventListener("resize", resize);

function stepSim() {
  // 반발 + 스프링 + 중심 인력. 노드 수십 개 규모라 O(n^2) 로 충분하다.
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
    // 화살촉.
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

// --- 메인 --------------------------------------------------------------------

#[tokio::test]
#[ignore = "로컬 Ollama 필요 - 온톨로지 구축(적재 품질) 수동 eval"]
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
        eprintln!("[skip] Ollama 에 연결 불가({base}) - `ollama serve` 후 재실행");
        return;
    }

    let mut results = Vec::new();
    for model in &models {
        eprintln!("\n=== 모델: {model} ===");
        results.push(build_ontology(&http, &base, model).await);
    }

    eprintln!("\n=== 비교 (observe / 노드 / 엣지 / 관계타입 / 커버리지 / 고립 / 중복쌍) ===");
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

    // 하네스 건전성 가드: 어떤 모델도 청크 하나 못 적재하면 브리지/표면이 깨진 것.
    assert!(
        results.iter().any(|r| r.observed() > 0),
        "어떤 모델도 observe 를 한 번도 성공시키지 못함 - 브리지/표면 점검 필요"
    );
}
