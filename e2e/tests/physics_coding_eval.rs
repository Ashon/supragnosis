//! Physics coding eval: 공통 온톨로지 위임이 소형 모델의 "코딩 결과물"을 개선하는가.
//!
//! delegation_eval(회수 QA), ontology_build_eval(적재)과 달리, 이 하네스는 위임의
//! 최종 목적 - 더 나은 작업 산출물 - 을 직접 잰다. 사람(하네스 작성자)이 큐레이션한
//! 고품질 공통 온톨로지("팀의 설계 결정")를 지식 베이스에 적재해 두고, 모델에게
//! 웹에서 도는 2D 물리 데모(자체 포함 HTML) 구현을 시킨다:
//!
//!   - bare      조건: 과제만 준다. 설계 결정은 모른다.
//!   - delegated 조건: 같은 과제 + MCP 도구. "팀 설계는 지식 베이스에 있다 - 조회하고
//!     따르라"고 지시한다.
//!
//! 설계 결정에는 지식 베이스에만 존재하는 구체 수치/공식(중력 900, restitution 0.8,
//! damping 0.999, dt 1/60, 충격량 공식의 (1+e) 항, invMass, 위치 보정)을 심는다.
//! 생성된 코드에 이 지문(fingerprint)이 나타나면 지식이 MCP 를 타고 코드로 흘러간
//! 직접 증거다 - bare 조건은 원리적으로 맞출 수 없다(추측 일치는 기저율로 드러난다).
//!
//! 정량 지표 (모델 x 조건):
//!   - 설계 준수 지문 7종 히트 수 (지식 위임의 핵심 신호 - 단, "지식이 옮겨졌다"의
//!     척도지 "코드가 맞다"의 척도가 아니다. 실측: 7/7 준수인데 공이 안 움직인 사례)
//!   - 행동 채점: 헤드리스 실행으로 그려진 궤적을 보고 움직임/바운스/경계유지를 판정
//!   - JS 문법 유효성 (node --check, node 없으면 skip)
//!   - 산출물 형식 준수 (단일 html 코드블록), 도구 호출 수, 토큰
//!
//! EVAL_REPLAY=1 이면 모델 호출 없이 저장된 데모 파일을 재채점한다(결정적).
//! EVAL_REPAIR_ROUNDS(기본 2): 실패 시 실패 내역(문법 stderr, 런타임 에러, 행동 미달)을
//! 같은 대화에 되먹여 고치게 하는 수리 라운드 수. 성공률이 최초 생성 대비 얼마나
//! 개선되는지가 지표다 - 위임 + 피드백 루프가 소형 모델의 조립 한계를 보완하는가.
//! 정성 산출물:
//!   - target/eval-reports/physics_demos/{model}_{condition}.html - 실제 실행 데모
//!   - target/eval-reports/physics_gallery.html - 전 데모를 나란히 놓은 갤러리
//!     (iframe srcdoc 임베드 - 로컬/웹 어디서든 자체 포함으로 열린다)
//!
//! 실행:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test physics_coding_eval -- --ignored --nocapture
//! 선택 env: OLLAMA_BASE_URL (기본 http://localhost:11434), OLLAMA_MODELS (기본 gemma4)
//!
//! Ollama 가 안 떠 있으면 조용히 통과(skip)한다.

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

// --- 공통 온톨로지 (사람이 큐레이션한 팀 설계 결정) ---------------------------

/// 설계 결정 한 건. content 가 지식의 본체, 엔티티/관계는 온톨로지 골격.
struct Decision {
    content: &'static str,
    entities: &'static [(&'static str, &'static str)],
    relations: &'static [(&'static str, &'static str, &'static str)],
}

/// 팀의 물리 데모 설계 결정. 구현에 필요한 수치/공식/순서를 전부 담는다 - 이 내용은
/// 프롬프트에 절대 싣지 않으며, delegated 조건의 모델만 MCP 조회로 얻을 수 있다.
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

/// 설계 지문: 코드에 이 패턴이 보이면 해당 설계 결정이 코드로 흘러간 것.
/// (이름, 정규식) - bare 조건이 우연히 맞출 확률이 낮은 구체 수치/구조를 고른다.
const FINGERPRINTS: [(&str, &str); 7] = [
    ("gravity 900", r"900"),
    ("restitution 0.8", r"0\.8"),
    ("damping 0.999", r"0\.999"),
    ("dt 1/60", r"1\s*/\s*60|0\.01666|0\.0167"),
    ("impulse (1+e)", r"\(\s*1(\.0)?\s*\+\s*[a-zA-Z_.]*(e|rest)"),
    ("invMass", r"(?i)inv_?mass"),
    ("positional correction", r"(?i)penetrat|correction"),
];

// --- Ollama 브리지 (delegation_eval.rs 와 같은 패턴) --------------------------

// --- 과제와 실행 -------------------------------------------------------------

const TASK: &str = "Implement a small 2D physics demo as ONE self-contained HTML file: balls \
    bouncing under gravity inside a canvas, with ball-to-ball collisions properly resolved. \
    No external libraries. Reply with a single ```html code block containing the complete file \
    and nothing else after it.";

/// 한 (모델, 조건) 실행 결과. 수리 라운드가 있으면 최종 라운드의 평가가 실린다.
struct CodeResult {
    model: String,
    condition: &'static str,
    /// 추출한 HTML (코드블록). 없으면 형식 위반.
    html: Option<String>,
    fingerprints: Vec<(&'static str, bool)>,
    syntax_ok: Option<bool>, // None = node 없음 or html 없음
    /// 행동 채점(헤드리스 실행). None = node 없음 or html 없음.
    behavior: Option<Behavior>,
    search_calls: usize,
    tokens: u64,
    error: Option<String>,
    /// 라운드별 판정 라벨 (r0 = 최초 생성, r1+ = 수리 라운드).
    rounds: Vec<String>,
    /// 최초 생성(r0)이 성공이었는가 - 수리 이득의 기준선.
    initial_success: bool,
    /// 최종 성공 여부: html + 문법 + 행동(구동/움직임/바운스/경계) 전항 통과.
    success: bool,
}

impl CodeResult {
    fn fp_hits(&self) -> usize {
        self.fingerprints.iter().filter(|(_, h)| *h).count()
    }

    /// "2/3에 성공" / "3/3 실패" 형태의 라운드 요약.
    fn rounds_label(&self) -> String {
        if self.success {
            format!("{}/{}에 성공", self.rounds.len(), self.rounds.len())
        } else {
            format!("{}/{} 실패", self.rounds.len(), self.rounds.len())
        }
    }
}

/// 응답 텍스트에서 html 코드블록(또는 doctype 휴리스틱)을 뽑는다.
fn extract_html(text: &str) -> Option<String> {
    // ```html ... ``` 우선, 없으면 ``` ... ``` 안에 <canvas 가 있으면 수용.
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
    // 코드블록 없이 통짜 html 을 뱉는 모델 수용.
    if text.contains("<canvas") && (text.contains("<html") || text.contains("<!doctype") || text.contains("<!DOCTYPE")) {
        return Some(text.trim().to_string());
    }
    None
}

/// html 에서 모든 script 본문을 이어붙여 뽑는다. script 태그가 없거나 깨졌으면 None.
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

/// script 본문을 node --check 로 문법 검사한다. node 가 없으면 None.
/// (ok, 실패 시 stderr 앞부분) - stderr 는 수리 라운드 피드백에 쓴다.
fn check_syntax(html: &str, tmp: &std::path::Path) -> Option<(bool, String)> {
    let Some(script) = extract_script(html) else {
        return Some((false, "no <script> body found".into())); // 스크립트 없는 "데모"는 실패.
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

// --- 행동 채점 (runtime scoring) ---------------------------------------------
//
// 지문 검사는 "지식이 코드로 옮겨졌는가"만 잰다 - 조립 로직 버그(적용 순서, 단위 혼동)는
// 실행해야 드러난다(실측: gemma4 delegated 가 7/7 준수인데 중력을 적용 직후 리셋해 공이
// 안 움직였다). 그래서 node 에서 DOM/canvas 를 스텁하고 ctx.arc(x,y,r) 호출을 프레임별로
// 기록해, 데모 내부 구현과 무관하게 "그려진 공의 궤적"으로 행동을 채점한다.

/// node 용 헤드리스 러너. rAF/setInterval 을 3600 프레임(60초 상당) 구동하며 arc 좌표를
/// 수집해 JSON 으로 출력한다. 알 수 없는 DOM 접근은 블랙홀 프록시로 흡수한다.
/// 관측 창이 60초인 이유(실측): 시간 스케일을 잘못 잡은 생성 코드는 낙하에 수십 초가
/// 걸린다 - 4초 창에서는 "느리게나마 튀는" 데모가 정지로 오판됐다.
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

/// 행동 채점 결과. `ran=false` 면 실행 자체가 실패(런타임 에러/프레임 부족).
#[derive(Clone)]
struct Behavior {
    ran: bool,
    /// 공들이 실제로 움직였는가 (y 이동 범위 중앙값 > 15px).
    moved: bool,
    /// 표본의 95% 이상이 캔버스(여유 60px) 안에 있는가.
    contained: bool,
    /// 첫 바운스 관측 시각(시뮬레이션 초). 스펙대로면 1초 안이어야 한다.
    first_bounce_s: Option<f64>,
    note: String,
}

impl Behavior {
    fn label(&self) -> String {
        if !self.ran {
            return format!("실행 실패({})", self.note);
        }
        let bounce = match self.first_bounce_s {
            Some(s) => format!("[o] ~{s:.1}s"),
            None => "[x]".to_string(),
        };
        format!(
            "움직임 {} / 바운스 {bounce} / 경계 {}",
            if self.moved { "[o]" } else { "[x]" },
            if self.contained { "[o]" } else { "[x]" }
        )
    }
}

/// 러너를 돌려 프레임별 arc 좌표를 얻고 행동 지표를 계산한다. node 가 없으면 None.
/// 러너 프레임 루프는 유계지만 생성 코드의 자체 무한 루프에 대비해 10초 타임아웃을 건다.
fn run_behavior(html: &str, tmp: &std::path::Path) -> Option<Behavior> {
    let script = match extract_script(html) {
        Some(s) => s,
        None => {
            return Some(Behavior {
                ran: false,
                moved: false,
                contained: false,
                first_bounce_s: None,
                note: "script 없음".into(),
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
    // std 만으로 타임아웃: 별도 스레드가 wait 하고, 본선은 10초까지 기다린다.
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
                note: "타임아웃(무한 루프 의심)".into(),
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

    // 공 수가 가장 흔한 프레임만 궤적으로 쓴다(초기화/이펙트 프레임 잡음 제거).
    let nb = frames.iter().map(Vec::len).max().unwrap_or(0);
    let stable: Vec<&Vec<(f64, f64, f64)>> =
        frames.iter().filter(|f| f.len() == nb && nb > 0).collect();
    if stable.len() < 10 {
        return fail("프레임 부족(애니메이션 루프 미구동)");
    }

    let mut ranges = Vec::new();
    let mut bounce_frame: Option<usize> = None;
    for ball in 0..nb {
        let ys: Vec<f64> = stable.iter().map(|f| f[ball].1).collect();
        let (min, max) = ys
            .iter()
            .fold((f64::MAX, f64::MIN), |(lo, hi), y| (lo.min(*y), hi.max(*y)));
        ranges.push(max - min);
        // 낙하(누적 +30px) 후 상승(-15px) 패턴 = 바운스. 첫 관측 프레임을 기록한다
        // (시간 스케일 버그 진단용 - 스펙대로면 1초 안, 수십 초면 dt 축소 의심).
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
        Some(f) => format!(", 첫 바운스 ~{:.1}s", f as f64 / 60.0),
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

/// 한 라운드 산출물의 평가: 형식/지문/문법/행동.
struct Evaluation {
    html: Option<String>,
    fingerprints: Vec<(&'static str, bool)>,
    syntax: Option<(bool, String)>,
    behavior: Option<Behavior>,
}

impl Evaluation {
    /// 성공 = html 존재 + 문법 통과(측정 가능 시) + 행동 전항(구동/움직임/바운스/경계).
    fn success(&self) -> bool {
        self.html.is_some()
            && !matches!(self.syntax, Some((false, _)))
            && self
                .behavior
                .as_ref()
                .is_some_and(|b| b.ran && b.moved && b.first_bounce_s.is_some() && b.contained)
    }

    /// 라운드 로그용 짧은 판정 라벨.
    fn label(&self) -> String {
        if self.html.is_none() {
            return "html 없음".into();
        }
        if let Some((false, _)) = &self.syntax {
            return "문법 오류".into();
        }
        match &self.behavior {
            Some(b) if !b.ran => format!("실행 실패({})", b.note),
            Some(b) if self.success() => {
                format!("성공 (바운스 ~{:.1}s)", b.first_bounce_s.unwrap_or(0.0))
            }
            Some(b) => format!(
                "행동 미달 (움직임 {} 바운스 {} 경계 {})",
                if b.moved { "o" } else { "x" },
                if b.first_bounce_s.is_some() { "o" } else { "x" },
                if b.contained { "o" } else { "x" }
            ),
            None => "행동 미측정".into(),
        }
    }

    /// 수리 라운드 피드백: 실패 항목을 구체적으로 적어 고치게 한다.
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
        format!(
            "Your demo was tested automatically and it FAILED:\n- {}\n\nFix the problem and \
             reply again with ONE complete ```html code block containing the whole corrected \
             file (not a diff).",
            issues.join("\n- ")
        )
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

/// 대화 히스토리에서 어시스턴트 텍스트 답 하나를 받아낸다.
/// delegated(tools=Some)는 tool_calls 를 실행-되먹임하며 텍스트가 나올 때까지 돈다.
/// 받은 어시스턴트 메시지는 히스토리에 push 해 수리 라운드가 같은 대화를 잇게 한다.
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
            return Err("도구 없는 조건에서 tool_calls 발생".into());
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
    Err(format!("{MAX_ROUNDS} 도구 왕복 내에 텍스트 답 없음"))
}

/// 한 (모델, 조건) 실행: 최초 생성 + 실패 시 최대 `repair_rounds` 회 수리.
/// 실패 내역(문법 stderr, 런타임 에러, 행동 미달)을 되먹여 같은 대화에서 고치게 한다.
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
        format!(
            "{TASK}\n\nIMPORTANT: your team's agreed design decisions for this demo (step \
             order, integration method, exact constants, collision and impulse formulas, \
             rendering spec) are stored in the knowledge base. Before writing code, use the \
             search_knowledge tool (several queries, e.g. \"step order\", \"impulse\", \
             \"restitution\", \"gravity\", \"rendering\") to retrieve the design, then follow \
             it exactly in your implementation."
        )
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
                // 전송/프로토콜 오류는 피드백으로 고칠 수 없다 - 중단.
                rounds.push(format!("r{r}: 오류 - {e}"));
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
        .is_some_and(|r0| r0.contains("성공"));
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

/// 공통 온톨로지를 엔진에 적재한다.
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
            .expect("설계 결정 적재");
    }
}

// --- 갤러리/리포트 -----------------------------------------------------------

fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;")
}

/// 전 데모를 iframe(srcdoc) 그리드로 나란히 놓는 자체 포함 갤러리.
fn render_gallery(results: &[CodeResult]) -> String {
    let mut cells = String::new();
    for r in results {
        let badge = match (&r.html, r.syntax_ok) {
            (None, _) => "형식 위반 - html 코드블록 없음".to_string(),
            (_, Some(false)) => "js 문법 오류".to_string(),
            _ => {
                let behavior = r
                    .behavior
                    .as_ref()
                    .map(|b| b.label())
                    .unwrap_or_else(|| "행동 미측정".into());
                format!(
                    "{} / 설계 준수 {}/{} / 검색 {}회 / {behavior}",
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
                let why = r.error.as_deref().unwrap_or("응답에서 html 을 찾지 못함");
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
  /* 데모 캔버스(500~600px 높이)가 잘리지 않게: 2 배 가상 뷰포트를 0.5 로 축소해 담는다. */
  .frame {{ width: 100%; aspect-ratio: 3 / 2; overflow: hidden; position: relative;
           border-radius: 6px; background: #14161a; }}
  .frame iframe {{ width: 200%; height: 200%; transform: scale(0.5); transform-origin: 0 0;
                  border: 0; position: absolute; left: 0; top: 0; }}
  .empty {{ aspect-ratio: 3 / 2; display: grid; place-items: center; color: #7f6a6a;
           font-size: 12.5px; border: 1px dashed #3a2f2f; border-radius: 6px; padding: 12px;
           text-align: center; }}
</style>
<h1>physics coding eval - 데모 갤러리</h1>
<p class="sub">bare = 과제만 / delegated = 공통 온톨로지를 MCP 로 조회. "설계 준수 n/7"은
지식 베이스에만 있는 설계 지문(중력 900, restitution 0.8, damping 0.999, dt 1/60,
충격량 (1+e), invMass, 위치 보정)이 코드에 나타난 수.</p>
<div class="grid">
{cells}</div>
"##
    )
}

fn render_markdown(results: &[CodeResult]) -> String {
    let mut md = String::new();
    md.push_str("# physics coding eval 리포트\n\n");
    md.push_str("공통 온톨로지(팀 설계 결정) 위임이 코딩 산출물에 반영되는가.\n\n");
    let initial_ok = results.iter().filter(|r| r.initial_success).count();
    let final_ok = results.iter().filter(|r| r.success).count();
    md.push_str(&format!(
        "**성공률: 최초 생성 {initial_ok}/{} -> 수리 후 {final_ok}/{}** \
         (성공 = 문법 + 구동 + 움직임 + 바운스 + 경계 전항 통과)\n\n",
        results.len(),
        results.len()
    ));
    md.push_str("| 모델 | 조건 | 라운드 | js 문법 | 설계 준수 | 행동 | 검색 | 토큰 |\n");
    md.push_str("|---|---|---|---|---|---|---|---|\n");
    for r in results {
        md.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {} | {} | {} |\n",
            r.model,
            r.condition,
            r.rounds_label(),
            match r.syntax_ok {
                Some(true) => "ok",
                Some(false) => "오류",
                None => "-",
            },
            r.fp_hits(),
            r.fingerprints.len(),
            r.behavior.as_ref().map(|b| b.label()).unwrap_or_else(|| "-".into()),
            r.search_calls,
            r.tokens
        ));
    }
    md.push_str("\n## 라운드 로그 (수리 수렴 과정)\n\n");
    for r in results {
        md.push_str(&format!(
            "- {} / {}: {}\n",
            r.model,
            r.condition,
            r.rounds.join(" -> ")
        ));
    }
    md.push_str("\n## 지문 상세\n\n");
    for r in results {
        let hits: Vec<String> = r
            .fingerprints
            .iter()
            .map(|(n, h)| format!("{}{}", if *h { "[o] " } else { "[x] " }, n))
            .collect();
        md.push_str(&format!("- {} / {}: {}\n", r.model, r.condition, hits.join(", ")));
        if let Some(e) = &r.error {
            md.push_str(&format!("  - 오류: {e}\n"));
        }
    }
    md
}

// --- 메인 --------------------------------------------------------------------

#[tokio::test]
#[ignore = "로컬 Ollama 필요 - 온톨로지 위임 코딩 산출물 수동 eval"]
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
    std::fs::create_dir_all(&tmp).expect("데모 디렉터리 생성");
    let mut results: Vec<CodeResult> = Vec::new();

    // EVAL_REPLAY=1: 모델 호출 없이 저장된 데모 파일들을 다시 채점한다(지문 + 문법 + 행동).
    // 사람이 갤러리에서 본 바로 그 산출물을 결정적으로 재채점할 때 쓴다.
    if std::env::var("EVAL_REPLAY").as_deref() == Ok("1") {
        let mut names: Vec<_> = std::fs::read_dir(&tmp)
            .expect("physics_demos 읽기")
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
            let html = std::fs::read_to_string(tmp.join(&name)).expect("데모 읽기");
            // 저장본은 이미 순수 html 이라 코드블록으로 감싸 동일 평가 경로를 태운다.
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
                rounds: vec![format!("replay: {}", if success { "성공" } else { "실패" })],
                initial_success: success,
                success,
            };
            eprintln!(
                "  [replay] {model:<16} {condition:<10} 설계 준수 {}/{}  {}",
                r.fp_hits(),
                r.fingerprints.len(),
                r.behavior.as_ref().map(|b| b.label()).unwrap_or_else(|| "-".into())
            );
            results.push(r);
        }
        report::write_report("physics_gallery.html", &render_gallery(&results));
        report::write_report("physics_coding_eval.md", &render_markdown(&results));
        eprintln!("\n[gallery] {}", dir.join("physics_gallery.html").display());
        assert!(!results.is_empty(), "재채점할 데모 파일이 없음");
        return;
    }

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("http client");

    if !ollama_reachable(&http, &base).await {
        eprintln!("[skip] Ollama 에 연결 불가({base}) - `ollama serve` 후 재실행");
        return;
    }

    let repair_rounds: usize = std::env::var("EVAL_REPAIR_ROUNDS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    eprintln!("[config] 수리 라운드 최대 {repair_rounds}회 (EVAL_REPAIR_ROUNDS)");

    for model in &models {
        eprintln!("\n=== 모델: {model} ===");

        // 공통 온톨로지를 실은 MCP 서버 (모델마다 새로 - 상태 격리).
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
                "  [{condition:<9}] {}  설계 준수 {}/{}  검색 {}회  ({}tk)",
                r.rounds_label(),
                r.fp_hits(),
                r.fingerprints.len(),
                r.search_calls,
                r.tokens
            );
            if let Some(h) = &r.html {
                let safe_model = model.replace(['/', ':'], "_");
                let path = tmp.join(format!("{safe_model}_{condition}.html"));
                std::fs::write(&path, h).expect("데모 저장");
            }
            results.push(r);
        }

        let _ = client.cancel().await;
        let _ = server.await;
    }

    eprintln!("\n=== 비교 (설계 준수 = 지식이 코드로 흘러간 지문 수) ===");
    for r in &results {
        eprintln!(
            "  {:<14} {:<10} 설계 준수 {}/{}  검색 {}회",
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

    // 하네스 건전성 가드: 어떤 (모델, 조건)도 html 을 못 내면 브리지/추출이 깨진 것.
    assert!(
        results.iter().any(|r| r.html.is_some()),
        "어떤 모델도 html 데모를 내지 못함 - 브리지/추출 점검 필요"
    );
}
