//! supragnosis-viz - 온톨로지 라이브 시각화(localhost HTTP 뷰어).
//!
//! MCP 도구 표면(원칙 21)과는 별개인 **사람용 읽기 채널**이다. 서버 프로세스 안에
//! 얹혀 같은 `Arc<Engine>` 을 공유하므로(cozo/RocksDB 단일 프로세스 제약), 별도
//! 프로세스로 db 를 여는 lock 충돌 없이 `engine.graph()` 프로젝션을 그대로 노출한다.
//!
//! - `GET /` -> 자기완결 canvas 뷰어(외부 CDN 0). 몇 초마다 `/api/graph` 를 폴링해 갱신.
//! - `GET /api/graph[?workspace=<ws>]` -> `engine.graph(ws)` JSON(원칙 16: 결정적 정렬).
//!
//! 순수 읽기다 - 관측 로그를 건드리지 않는다(원칙 1). 바인딩은 loopback 전용으로
//! 강제해 원격 노출을 막는다(원칙 17: 지식 주권, 공유 가드 전까지 로컬 신뢰 표면 한정).

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use supragnosis_engine::{Engine, EventEnvelope, EventSink};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

/// MCP 이벤트를 브라우저(SSE)로 흘려보내는 [`EventSink`] 어댑터. 엔진에 붙으면 도구
/// 호출이 여기로 발행되고, `/api/events` SSE 커넥션들이 broadcast 로 구독한다.
/// 수신자(열린 뷰어)가 없으면 send 는 무시된다 - 관측가능성은 선택(원칙 19의 정신).
pub struct BroadcastSink {
    tx: broadcast::Sender<String>,
}

impl BroadcastSink {
    pub fn new(tx: broadcast::Sender<String>) -> Self {
        Self { tx }
    }
}

impl EventSink for BroadcastSink {
    fn emit(&self, env: &EventEnvelope) {
        // 동기 컨텍스트(도구 핸들러)에서 호출된다 - send 는 비블로킹. 직렬화 실패/수신자
        // 없음은 조용히 버린다(뷰어가 안 열려 있어도 도구 동작에 영향 없어야 한다).
        if let Ok(json) = serde_json::to_string(env) {
            let _ = self.tx.send(json);
        }
    }
}

/// 요청 라인 + 헤더를 읽어들이는 상한(바이트). GET 전용이라 바디는 없고, 이 상한을
/// 넘으면 악의적/비정상 요청으로 보고 끊는다.
const MAX_REQUEST_HEAD: usize = 16 * 1024;

/// `SUPRAGNOSIS_VIZ_ADDR` 를 파싱하고 **loopback 인지 검증**한다 (원칙 17).
///
/// `host:port` IP 리터럴만 받는다(예: `127.0.0.1:7373`). 비로opback 주소는 거부한다 -
/// 원격 노출은 sync 경계의 공유 가드가 생기기 전까지 허용되지 않는다. 호스트명(localhost)은
/// DNS 해석을 요구하므로 받지 않는다(모호함 제거).
pub fn parse_local_addr(s: &str) -> anyhow::Result<SocketAddr> {
    let addr: SocketAddr = s.trim().parse().with_context(|| {
        format!("잘못된 SUPRAGNOSIS_VIZ_ADDR: {s:?} - IP:포트 형식이어야 한다 (예: 127.0.0.1:7373)")
    })?;
    if !addr.ip().is_loopback() {
        anyhow::bail!(
            "SUPRAGNOSIS_VIZ_ADDR {addr} 는 loopback 이 아니다 - 뷰어는 비로컬 바인드를 \
             거부한다(원칙 17: 지식 주권). 127.0.0.1:<port> 를 사용하라"
        );
    }
    Ok(addr)
}

/// 주입된 리스너에서 커넥션을 받아 뷰어/그래프 API 를 서빙한다(무한 accept 루프).
///
/// 바인딩은 **호출자가** 한다(테스트가 포트 0 으로 바인드해 실제 포트를 조회할 수 있게).
/// 커넥션마다 태스크로 분리하되, 개별 커넥션 실패는 삼켜 서버를 죽이지 않는다.
pub async fn serve(
    engine: Arc<Engine>,
    listener: TcpListener,
    events: broadcast::Sender<String>,
) -> anyhow::Result<()> {
    loop {
        let (stream, _peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!(error = %e, "viz accept 실패 - 계속");
                continue;
            }
        };
        let engine = Arc::clone(&engine);
        let events = events.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_conn(&engine, &events, stream).await {
                tracing::debug!(error = %e, "viz 커넥션 처리 실패");
            }
        });
    }
}

/// 한 커넥션: 요청 라인만 파싱(헤더/바디 무시) -> 라우팅 -> 응답 후 close.
/// 예외로 `/api/events` 는 SSE 스트림이라 닫지 않고 이벤트를 계속 흘린다.
async fn handle_conn(
    engine: &Engine,
    events: &broadcast::Sender<String>,
    mut stream: TcpStream,
) -> anyhow::Result<()> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > MAX_REQUEST_HEAD {
            break;
        }
    }

    let head = String::from_utf8_lossy(&buf);
    let request_line = head.lines().next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    // SSE: 라이브 MCP 이벤트 스트림 - 응답을 닫지 않고 이벤트를 계속 흘린다.
    if method == "GET" && path == "/api/events" {
        return stream_events(stream, events.subscribe()).await;
    }

    let resp = route(engine, method, path, query);
    write_response(&mut stream, &resp).await
}

/// SSE 스트림: `text/event-stream` 헤더 후 이벤트마다 `data: {json}\n\n` 를 흘린다.
/// JSON 은 한 줄이라 프레임이 단순하다. 클라이언트가 끊기면(write 실패) 종료.
async fn stream_events(
    mut stream: TcpStream,
    mut rx: broadcast::Receiver<String>,
) -> anyhow::Result<()> {
    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n: ok\n\n",
        )
        .await?;
    stream.flush().await?;
    loop {
        match rx.recv().await {
            Ok(json) => {
                let frame = format!("data: {json}\n\n");
                if stream.write_all(frame.as_bytes()).await.is_err() {
                    break; // 클라이언트 끊김
                }
                if stream.flush().await.is_err() {
                    break;
                }
            }
            // 느린 클라이언트가 뒤처지면 유실분은 건너뛰고 계속.
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}

/// (status line, content-type, body) - 응답의 결정된 3요소.
struct Response {
    status: &'static str,
    content_type: &'static str,
    body: String,
}

fn route(engine: &Engine, method: &str, path: &str, query: &str) -> Response {
    if method != "GET" {
        return Response {
            status: "405 Method Not Allowed",
            content_type: "application/json",
            body: err_body("only GET is supported"),
        };
    }
    match path {
        "/" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: VIEWER_HTML.to_string(),
        },
        "/api/graph" => graph_response(engine, query),
        "/api/workspaces" => workspaces_response(engine),
        _ => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body("unknown path - try /, /api/graph, /api/workspaces, or /api/events"),
        },
    }
}

/// `/api/graph` - 쿼리의 workspace 를 해석해 그래프 프로젝션을 낸다.
/// - 미지정 -> 노드 기본 워크스페이스(스코프된 뷰)
/// - `*` / `all` / 빈 값 -> 전체(None)
///
/// 저장소 고장은 500 + 에러 바디(원칙 5: 고장은 빈 그래프가 아니다).
fn graph_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.graph(ws_owned.as_deref()) {
        Ok(graph) => match serde_json::to_string(&graph) {
            Ok(json) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: json,
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty graph (원칙 5)"
            })
            .to_string(),
        },
    }
}

/// `/api/workspaces` - 지식이 있는 워크스페이스 목록(정렬, 원칙 16). 뷰어의 워크스페이스
/// 피커가 소비한다 - 이름을 직접 타이핑하지 않고 클릭으로 고르게 한다. 고장은 500(원칙 5).
fn workspaces_response(engine: &Engine) -> Response {
    match engine.workspaces() {
        Ok(list) => match serde_json::to_string(&list) {
            Ok(json) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: json,
            },
            Err(e) => Response {
                status: "500 Internal Server Error",
                content_type: "application/json",
                body: err_body(&format!("serialize error: {e}")),
            },
        },
        Err(e) => Response {
            status: "500 Internal Server Error",
            content_type: "application/json",
            body: serde_json::json!({
                "error": e.to_string(),
                "note": "storage backend failure - NOT an empty list (원칙 5)"
            })
            .to_string(),
        },
    }
}

async fn write_response(stream: &mut TcpStream, r: &Response) -> anyhow::Result<()> {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n",
        r.status,
        r.content_type,
        r.body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(r.body.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

fn err_body(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// 최소 퍼센트 디코딩(`%XX` + `+` -> 공백). 워크스페이스 이름의 공백/특수문자를 위해.
/// 잘못된 시퀀스는 원문 그대로 둔다(관용적).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 자기완결 라이브 뷰어(외부 CDN 0). canvas 그래프 탐색기: 줌/팬, hover 이웃 하이라이트,
/// 클릭 포커스/핀, 검색, fit-to-view, 타입 범례 필터, 라벨 정리. 색은 dataviz 스킬의
/// 검증된 dark 카테고리 팔레트(고정 순서, 순환 대신 9번째부터 "other"). alpha 냉각 +
/// 반지름 기반 충돌 분리로 겹침을 막는다. `/api/graph` 를 주기 폴링해 라이브 갱신하고,
/// 노드 위치는 폴링 간 id 기준으로 유지해 화면이 튀지 않는다.
const VIEWER_HTML: &str = r###"<!doctype html>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>supragnosis ontology viewer</title>
<style>
  :root {
    color-scheme: dark;
    --surface:#1a1a19; --ink:#ffffff; --ink2:#c3c2b7; --muted:#898781;
    --line:#4a4a46; --line-hi:#8fb4e6; --border:rgba(255,255,255,0.10); --accent:#3987e5;
  }
  * { box-sizing:border-box; }
  html,body { margin:0; height:100%; }
  body { background:var(--surface); color:var(--ink2); overflow:hidden;
         font:13px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif; }
  canvas { display:block; position:fixed; inset:0; cursor:grab; }
  canvas.grabbing { cursor:grabbing; }
  header { position:fixed; top:0; left:0; right:0; z-index:5; padding:9px 14px;
           display:flex; gap:9px; align-items:center; flex-wrap:wrap;
           background:var(--surface); border-bottom:1px solid var(--border); }
  h1 { font-size:14px; margin:0 6px 0 0; font-weight:600; color:var(--ink); }
  input,button { background:#22221f; border:1px solid var(--border); color:var(--ink);
                 padding:5px 9px; border-radius:7px; font:inherit; font-size:12.5px; }
  input::placeholder { color:var(--muted); }
  button { cursor:pointer; } button:hover { border-color:var(--line-hi); }
  .hint { color:var(--muted); font-size:12px; }
  #status { color:var(--muted); font-size:12px; margin-left:auto; white-space:nowrap; }
  #chrome { position:fixed; top:47px; left:0; right:0; z-index:5; padding:6px 14px 0;
            display:flex; flex-direction:column; gap:5px; pointer-events:none; }
  #chrome > * { pointer-events:auto; }
  #wschips,#legend { display:flex; gap:6px; flex-wrap:wrap; align-items:center; }
  .lbl { color:var(--muted); font-size:11.5px; margin-right:2px; }
  .chip,.lg { padding:2px 9px; border-radius:11px; background:#22221f; border:1px solid var(--border);
              cursor:pointer; font-size:12px; color:var(--ink2); user-select:none; }
  .chip.on { background:#2b3a52; border-color:var(--accent); color:var(--ink); }
  .lg { display:inline-flex; align-items:center; gap:6px; }
  .lg.off { opacity:0.38; }
  .sw { width:10px; height:10px; border-radius:3px; display:inline-block; }
  #stats { color:var(--muted); font-size:11.5px; }
  #tip { position:fixed; pointer-events:none; z-index:10; display:none; max-width:320px;
         background:#0d0d0df2; border:1px solid var(--border); border-radius:8px;
         padding:7px 10px; font-size:12.5px; color:var(--ink2); box-shadow:0 6px 20px #000a; }
  #tip b { color:var(--ink); }
  #tip .k { color:var(--muted); }
  #hud { position:fixed; right:12px; bottom:12px; z-index:5; display:flex; gap:6px; }
  #hud button { width:34px; height:34px; padding:0; font-size:16px; line-height:1;
                display:flex; align-items:center; justify-content:center; }
  #empty { position:fixed; inset:0; display:none; align-items:center; justify-content:center;
           color:var(--muted); font-size:13px; pointer-events:none; }
  #followBtn.on { background:#2b3a52; border-color:var(--accent); color:var(--ink); }
  #log { position:fixed; left:12px; bottom:12px; z-index:6; width:300px; max-width:42vw;
         display:flex; flex-direction:column; gap:4px; pointer-events:none; }
  #log .row { background:#0d0d0de6; border:1px solid var(--border); border-radius:7px;
              padding:4px 9px; font-size:11.5px; color:var(--ink2); animation:logfade 8s forwards; }
  #log .row b { color:var(--accent); font-weight:600; }
  #log .row .t { color:var(--muted); margin-right:5px; }
  @keyframes logfade { 0%{opacity:0;transform:translateY(6px);} 6%{opacity:1;transform:none;}
                       82%{opacity:1;} 100%{opacity:0;} }
  #detail { position:fixed; top:92px; right:12px; z-index:7; width:272px; max-width:44vw;
            max-height:calc(100vh - 150px); overflow-y:auto; display:none;
            background:#0d0d0df2; border:1px solid var(--border); border-radius:10px;
            padding:11px 13px; font-size:12.5px; color:var(--ink2); box-shadow:0 8px 28px #000a; }
  #detail.on { display:block; }
  #detail h2 { font-size:14px; margin:0 22px 2px 0; color:var(--ink); font-weight:600; word-break:break-word; }
  #detail .meta { color:var(--muted); font-size:11.5px; margin-bottom:4px; }
  #detail .sec { color:var(--muted); font-size:10.5px; letter-spacing:.05em; text-transform:uppercase; margin:10px 0 3px; }
  #detail .row { display:flex; align-items:center; gap:6px; padding:3px 5px; border-radius:6px; cursor:pointer; }
  #detail .row:hover { background:#ffffff14; }
  #detail .row .rel { color:var(--muted); font-size:11px; white-space:nowrap; }
  #detail .row .nm { color:var(--ink); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  #detail .dot { width:9px; height:9px; border-radius:3px; flex:0 0 auto; display:inline-block; }
  #detail .close { position:absolute; top:9px; right:11px; cursor:pointer; color:var(--muted);
                   border:none; background:none; font-size:15px; line-height:1; padding:0; }
  #detail .close:hover { color:var(--ink); }
  #detail .empty { color:var(--muted); font-style:italic; padding:2px 5px; }
</style>
<canvas id="c"></canvas>
<div id="empty">no nodes in this workspace - observe knowledge, or pick another workspace</div>
<header>
  <h1>supragnosis ontology</h1>
  <input id="search" placeholder="search nodes" size="16" autocomplete="off">
  <label class="hint">ws <input id="ws" placeholder="(default)" size="11" autocomplete="off"></label>
  <span class="hint">*=all</span>
  <button id="reload">reload</button>
  <button id="followBtn" class="on" title="follow agent activity: workspace + camera">follow</button>
  <button id="clusterBtn" title="group by type; keep cross-group links visible">group</button>
  <span id="session" class="hint"></span>
  <span id="status"></span>
</header>
<div id="log"></div>
<div id="detail"></div>
<div id="chrome">
  <div id="wschips"></div>
  <div id="legend"></div>
  <div id="stats"></div>
</div>
<div id="tip"></div>
<div id="hud">
  <button id="zin" title="zoom in">+</button>
  <button id="zout" title="zoom out">-</button>
  <button id="fit" title="fit to view">fit</button>
</div>
<script>
"use strict";
// --- dataviz 검증 dark 카테고리 팔레트(고정 순서, 순환 금지) ------------------------
const PALETTE = ["#3987e5","#008300","#d55181","#c98500","#199e70","#d95926","#9085e9","#e66767"];
const OTHER = "#898781";   // 9번째 타입부터: 순환 대신 중립색(dataviz non-negotiable)
const EDGE = "#4a4a46", EDGE_HI = "#8fb4e6", EDGE_OLD = "#5a4a55";
const INK = "#ffffff", INK2 = "#c3c2b7", SURFACE = "#1a1a19";

const canvas = document.getElementById("c"), ctx = canvas.getContext("2d");
const tip = document.getElementById("tip"), statusEl = document.getElementById("status");
const wsInput = document.getElementById("ws"), searchEl = document.getElementById("search");
const chipBar = document.getElementById("wschips"), legendEl = document.getElementById("legend");
const emptyEl = document.getElementById("empty"), logEl = document.getElementById("log");
const detailEl = document.getElementById("detail");

let follow = true;               // 카메라가 최신 에이전트 활동 노드를 따라가는지
let clusterMode = false;         // 타입 그룹으로 분리 배치 + 그룹 간 연결/브리지 강조
const bridgeSet = new Set();     // 다른 타입과 연결된 노드 id(그룹을 잇는 연결 노드)
const pulses = new Map();        // id -> 남은 프레임(이벤트 노드 강조 링 애니메이션)
const CLUSTER_PULL = 0.03;       // 그룹 목표점으로 끌어당기는 힘(중심 인력보다 강하게)
let footprintSession = null;     // 현재 발자국이 속한 세션(대화)
const footprint = new Set();     // 이 세션이 만진 노드 id들 - 대화의 지식 발자국
let nodes = [], edges = [], typeColor = {};
const posById = new Map();       // id -> {x,y,vx,vy} - 폴링 간 레이아웃 안정
const typeOff = new Set();       // 범례에서 숨긴 타입
let spiralN = 0;
let drag = null, hover = null, focus = null;
let searchTerm = "";
// 카메라: cam=현재(그리기), camT=목표. 매 프레임 cam 을 camT 로 이징해 줌/팬/포커스/맞춤을
// 부드럽게 만든다(즉시 점프 제거). 좌표는 CSS 픽셀(마우스 이벤트와 동일계).
let DPR = 1;
const cam = { s: 1, x: 0, y: 0 }, camT = { s: 1, x: 0, y: 0 };
let panning = null, downPos = null, userMoved = false, firstData = true, needFit = false;

// --- force 시뮬레이션 (alpha 냉각 + 충돌 분리) ------------------------------------
let alpha = 1;
const ALPHA_DECAY = 0.0228, ALPHA_MIN = 0.02;
// 힘 base 파라미터. 큰 그래프일수록 넓게 퍼지도록 stepSim 에서 노드 수(spread)로 스케일한다.
const REPULSE = 7000, SPRING_LEN = 120, SPRING_K = 0.02;
const CENTER_BASE = 0.0015; // 중심 인력 base - 큰 그래프에선 약화(중앙 뭉침 방지)
const RANGE_BASE = 240;     // 반발 사거리 base - 큰 그래프에선 확대(더 넓게 밀어냄)
const COLLIDE_PAD = 16, DAMPING = 0.85;
const MIN_SEP = 12;        // 반발 분모 하한 - 근접 시 힘 폭발(튀어나감) 방지
const MAX_V = 30;          // 프레임당 최대 속도 base - 큰 그래프에선 상향
const MAX_PUSH = 6;        // 프레임당 노드별 충돌 변위 상한 - 허브 폭발 방지
function nodeRadius(n) { return 5 + Math.min(8, (n.degree || 0) * 1.4); }
// 시뮬레이션을 깨운다(discrete wakeup). 이벤트에서만 호출: 새 노드/삭제(applyGraph),
// 드래그, 포커스. 연속 조건(겹침)으로는 절대 호출하지 않는다 - 정착 후 무한 재가열 방지.
function wake(a = 0.7) { alpha = Math.max(alpha, a); }

// --- 카메라 (canvas 는 전체화면, 마우스는 client 좌표) ----------------------------
function toWorld(sx, sy) { return [(sx - cam.x) / cam.s, (sy - cam.y) / cam.s]; }
function easeCam() {
  const k = 0.22;
  cam.s += (camT.s - cam.s) * k; cam.x += (camT.x - cam.x) * k; cam.y += (camT.y - cam.y) * k;
  if (Math.abs(camT.s - cam.s) < 0.001) cam.s = camT.s;
  if (Math.abs(camT.x - cam.x) < 0.25) cam.x = camT.x;
  if (Math.abs(camT.y - cam.y) < 0.25) cam.y = camT.y;
}
// 커서 아래 월드 점을 고정한 채 목표 배율을 바꾼다(이징으로 부드럽게 수렴).
function zoomAt(sx, sy, f) {
  const wx = (sx - camT.x) / camT.s, wy = (sy - camT.y) / camT.s;
  camT.s = Math.max(0.15, Math.min(4, camT.s * f));
  camT.x = sx - wx * camT.s; camT.y = sy - wy * camT.s; userMoved = true;
}
const TOP_INSET = 96;   // 상단 헤더/크롬이 가리는 높이 - 센터링/맞춤에서 보정
// 노드를 화면 중앙으로 부드럽게(포커스-투-줌). 너무 축소돼 있으면 살짝 확대.
function centerOn(n) {
  camT.s = Math.min(2.5, Math.max(cam.s, 1.1));
  camT.x = innerWidth / 2 - n.x * camT.s;
  camT.y = (innerHeight + TOP_INSET) / 2 - n.y * camT.s; userMoved = true;
}

function assignColors() {
  const types = [...new Set(nodes.map(n => n.type))].sort();
  typeColor = {};
  types.forEach((t, i) => { typeColor[t] = i < PALETTE.length ? PALETTE[i] : OTHER; });
}

function applyGraph(g) {
  const seen = new Set();
  let added = false;
  nodes = g.nodes.map(n => {
    let p = posById.get(n.id);
    if (!p) {
      added = true;
      const i = spiralN++, a = i * 2.39996, r = 60 + 30 * Math.sqrt(i);
      p = { x: innerWidth/2 + r*Math.cos(a), y: innerHeight/2 + r*Math.sin(a), vx:0, vy:0 };
      posById.set(n.id, p);
    }
    seen.add(n.id);
    return Object.assign(p, n);
  });
  let removed = 0;
  for (const id of [...posById.keys()]) if (!seen.has(id)) { posById.delete(id); removed++; }
  if (added || removed) wake();

  const byId = Object.fromEntries(nodes.map(n => [n.id, n]));
  edges = g.edges.map(e => Object.assign({}, e, { a: byId[e.from], b: byId[e.to] }))
                 .filter(e => e.a && e.b);
  // 브리지 노드: 다른 타입(그룹)과 연결된 노드 - 그룹을 잇는 연결/네비게이션 지점.
  bridgeSet.clear();
  for (const e of edges) if (e.a.type !== e.b.type) { bridgeSet.add(e.a.id); bridgeSet.add(e.b.id); }
  assignColors();
  renderLegend();
  const s = g.stats || {};
  statusEl.textContent = "updated " + new Date().toLocaleTimeString();
  document.getElementById("stats").textContent =
    `nodes ${s.node_count ?? nodes.length} / edges ${s.edge_count ?? edges.length}`
    + (clusterMode ? ` / groups ${Object.keys(typeColor).length}, bridges ${bridgeSet.size}` : "")
    + (s.type_counts ? " / " + Object.entries(s.type_counts).map(([t,c]) => `${t} ${c}`).join(", ") : "");
  emptyEl.style.display = nodes.length ? "none" : "flex";

  // 포커스 중이면 상세 인스펙터를 갱신(연결 변화 반영). 포커스 노드가 사라졌으면 해제.
  if (focus) { if (nodes.includes(focus)) renderDetail(focus); else { focus = null; renderDetail(null); } }

  // 초기 자동 맞춤: 레이아웃이 정착(냉각 완료)한 뒤 한 번, 사용자 조작 전에만(draw 에서).
  if (firstData && nodes.length) { firstData = false; needFit = true; }
}

function renderLegend() {
  legendEl.innerHTML = "";
  const types = Object.keys(typeColor).sort();
  if (!types.length) return;
  const lbl = document.createElement("span"); lbl.className = "lbl"; lbl.textContent = "types:";
  legendEl.appendChild(lbl);
  for (const t of types) {
    const el = document.createElement("span");
    el.className = "lg" + (typeOff.has(t) ? " off" : "");
    const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = typeColor[t];
    el.appendChild(sw); el.appendChild(document.createTextNode(t));
    el.title = "click to toggle";
    el.onclick = () => { if (typeOff.has(t)) typeOff.delete(t); else typeOff.add(t); renderLegend(); };
    legendEl.appendChild(el);
  }
}

// hover/focus/검색으로 강조할 노드/엣지 집합. 없으면 null(전부 동일 강조).
function activeSet() {
  const anchor = focus || hover;
  if (anchor) {
    const ns = new Set([anchor.id]), es = new Set();
    for (let i = 0; i < edges.length; i++) {
      const e = edges[i];
      if (e.a.id === anchor.id || e.b.id === anchor.id) { es.add(i); ns.add(e.a.id); ns.add(e.b.id); }
    }
    return { ns, es };
  }
  if (searchTerm) {
    const ns = new Set();
    for (const n of nodes) if (n.name.toLowerCase().includes(searchTerm)) ns.add(n.id);
    return { ns, es: new Set() };
  }
  return null;
}

async function poll() {
  const ws = wsInput.value.trim();
  const url = "/api/graph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
  try {
    const r = await fetch(url, { cache: "no-store" });
    if (!r.ok) { statusEl.textContent = "HTTP " + r.status; return; }
    const g = await r.json();
    if (g.error) { statusEl.textContent = g.error; return; }
    applyGraph(g);
  } catch (e) { statusEl.textContent = "연결 실패 - 서버 실행 확인"; }
}

function currentWs() { return wsInput.value.trim(); }
function renderChipsActive() {
  const cur = currentWs();
  chipBar.querySelectorAll(".chip").forEach(c => c.classList.toggle("on", c.dataset.ws === cur));
}
async function loadWorkspaces() {
  try {
    const r = await fetch("/api/workspaces", { cache: "no-store" });
    if (!r.ok) return;
    const list = await r.json();
    const cur = currentWs();
    const mk = (label, val) => {
      const c = document.createElement("span");
      c.className = "chip" + (val === cur ? " on" : "");
      c.dataset.ws = val; c.textContent = label;
      c.onclick = () => { wsInput.value = val; renderChipsActive(); poll(); };
      return c;
    };
    const lbl = document.createElement("span"); lbl.className = "lbl"; lbl.textContent = "workspaces:";
    chipBar.replaceChildren(lbl, mk("(all)", "*"), ...list.map(w => mk(w, w)));
  } catch (e) { /* 서버 미기동 - 다음 주기에 재시도 */ }
}

// --- 라이브 MCP 활동(SSE) --------------------------------------------------------
function nodeById(id) { return nodes.find(n => n.id === id); }
function esc(s) { return String(s).replace(/[<&>]/g, c => ({ "<": "&lt;", "&": "&amp;", ">": "&gt;" }[c])); }
function pulseNodes(ids) { for (const id of ids || []) if (posById.has(id)) pulses.set(id, 60); }
function logRow(html) {
  const row = document.createElement("div");
  row.className = "row";
  row.innerHTML = `<span class="t">${new Date().toLocaleTimeString()}</span>${html}`;
  logEl.prepend(row);
  while (logEl.children.length > 8) logEl.lastChild.remove();
  setTimeout(() => row.remove(), 8000);
}
function primaryNode(ev) {
  const id = ev.kind === "observe" ? (ev.entities || [])[0]
    : ev.kind === "traverse" ? ev.start
    : ev.kind === "get_entity" ? (ev.found ? ev.id : null)
    : ev.kind === "search" ? (ev.nodes || [])[0] : null;
  return id ? nodeById(id) : null;
}
async function handleEvent(ev) {
  // 세션(대화)이 바뀌면 발자국 리셋 - 새 대화의 지식 사용을 처음부터 추적.
  if (ev.session && ev.session !== footprintSession) { footprintSession = ev.session; footprint.clear(); }
  // follow 중 활동이 다른 워크스페이스에서 일어나면 그쪽으로 전환한다 - 안 그러면 추가된
  // 노드/히트가 현재 스코프 밖이라 화면에 안 나타난다(SSE 이벤트는 와도 폴링 ws 불일치).
  const switched = follow && ev.workspace && currentWs() !== "*" && currentWs() !== ev.workspace;
  if (switched) { wsInput.value = ev.workspace; renderChipsActive(); }
  let ids = [];
  if (ev.kind === "observe") {
    logRow(`<b>observe</b> +${(ev.entities||[]).length} ent, +${ev.relations||0} rel <span class="t">ws ${esc(ev.workspace)}</span>`);
    await poll();                       // 새 노드가 그래프에 들어오길 기다렸다 펄스
    ids = ev.entities || [];
  } else if (ev.kind === "search") {
    logRow(`<b>search</b> "${esc(ev.query)}" -> ${ev.hits} hits <span class="t">${esc(ev.mode)}</span>`);
    if (switched) await poll();          // 워크스페이스 전환됐으면 그 그래프를 로드(히트 보이게)
    ids = ev.nodes || [];
  } else if (ev.kind === "get_entity") {
    logRow(`<b>get_entity</b> ${esc(ev.name || ev.id.slice(0,8))} <span class="t">${ev.found ? "found" : "unknown"}</span>`);
    ids = ev.found ? [ev.id] : [];
  } else if (ev.kind === "traverse") {
    const sn = nodeById(ev.start);
    logRow(`<b>traverse</b> ${esc(sn ? sn.name : ev.start.slice(0,8))} -> ${(ev.reached||[]).length}`);
    ids = [ev.start, ...(ev.reached || [])];
  } else return;
  pulseNodes(ids);
  for (const id of ids) if (id) footprint.add(id);   // 대화 발자국 누적(노드 존재 여부 무관)
  wake(0.3);
  if (follow) { const n = primaryNode(ev); if (n) centerOn(n); }
  const sEl = document.getElementById("session");
  if (sEl) sEl.textContent = footprintSession ? `session ${footprintSession.slice(0,22)} · ${footprint.size} used` : "";
}
function connectEvents() {
  try {
    const es = new EventSource("/api/events");
    es.onmessage = e => { try { handleEvent(JSON.parse(e.data)); } catch (_) {} };
    // 오류 시 EventSource 가 자동 재연결한다.
  } catch (_) { /* EventSource 미지원 - 폴링만으로 동작 */ }
}

// --- 상세 인스펙터: 클릭한 노드의 연결(이웃 + 관계)을 보여주고, 이웃 클릭으로 탐색 ---
function renderDetail(node) {
  if (!node) { detailEl.className = ""; detailEl.innerHTML = ""; return; }
  const outs = edges.filter(e => e.a === node && !typeOff.has(e.b.type));
  const ins = edges.filter(e => e.b === node && !typeOff.has(e.a.type));
  const rowHtml = (rel, other, dir) =>
    `<div class="row" data-id="${esc(other.id)}" title="focus ${esc(other.name)}">`
    + `<span class="dot" style="background:${typeColor[other.type] || OTHER}"></span>`
    + `<span class="rel">${dir} ${esc(rel)}</span>`
    + `<span class="nm">${esc(other.name)}</span></div>`;
  const list = (arr, dir) => arr.length
    ? arr.map(e => rowHtml(e.type, dir === "->" ? e.b : e.a, dir)).join("")
    : `<div class="empty">none</div>`;
  detailEl.innerHTML =
    `<button class="close" title="close">x</button>`
    + `<h2>${esc(node.name)}</h2>`
    + `<div class="meta"><span class="dot" style="background:${typeColor[node.type] || OTHER}"></span> `
    + `${esc(node.type)} / deg ${node.degree || 0} / src ${node.sources} / ${esc(String(node.trust_tier))}</div>`
    + `<div class="sec">outgoing (${outs.length})</div>${list(outs, "->")}`
    + `<div class="sec">incoming (${ins.length})</div>${list(ins, "<-")}`;
  detailEl.className = "on";
  detailEl.querySelector(".close").onclick = () => { focus = null; renderDetail(null); };
  detailEl.querySelectorAll(".row").forEach(r => {
    r.onclick = () => {
      const n = nodeById(r.dataset.id);
      if (n) { focus = n; wake(0.3); centerOn(n); renderDetail(n); }
    };
  });
}

// 슈퍼샘플링(HiDPI): 백킹 스토어를 DPR 배로 키우고 CSS 크기는 뷰포트로 고정 -> 선명.
function resize() {
  DPR = Math.min(window.devicePixelRatio || 1, 2);   // 성능 위해 2x 상한
  canvas.width = Math.round(innerWidth * DPR);
  canvas.height = Math.round(innerHeight * DPR);
  canvas.style.width = innerWidth + "px";
  canvas.style.height = innerHeight + "px";
}
addEventListener("resize", resize);

// 대상 노드 집합을 화면에 담도록 목표 카메라를 잡는다(이징으로 부드럽게). CSS 픽셀 기준.
function fitView(list, pad = 90) {
  const src = (list || nodes).filter(n => !typeOff.has(n.type));
  if (!src.length) return;
  let a = 1e9, b = 1e9, c = -1e9, d = -1e9;
  for (const n of src) { a = Math.min(a,n.x); b = Math.min(b,n.y); c = Math.max(c,n.x); d = Math.max(d,n.y); }
  const w = innerWidth, h = innerHeight, gw = Math.max(1, c-a), gh = Math.max(1, d-b);
  camT.s = Math.max(0.15, Math.min(2.5, Math.min((w - pad*2) / gw, (h - pad*2 - TOP_INSET) / gh)));
  camT.x = w/2 - (a+c)/2*camT.s;
  camT.y = (h + TOP_INSET)/2 - (b+d)/2*camT.s;
}

function stepSim() {
  const N = nodes.length;
  if (N === 0) return;
  const cooling = alpha >= ALPHA_MIN;
  if (cooling) alpha += (0 - alpha) * ALPHA_DECAY;
  const active = alpha >= ALPHA_MIN;   // 냉각 완료면 휴면 - 어떤 힘도 적용하지 않는다
  const pinned = v => v === drag || v === focus;

  // 노드 수로 스케일: 많을수록 넓게 퍼진다(반발 사거리/강도 up, 중심 인력 down).
  // hairball(중앙 뭉침) 방지 - base 는 소규모 기준, 대규모는 spread 로 확장.
  const spread = Math.min(4, Math.max(1, Math.sqrt(N / 20)));
  const range = RANGE_BASE * spread, centerG = CENTER_BASE / spread;
  const repulse = REPULSE * spread, maxV = MAX_V * Math.min(spread, 2);

  // 충돌 변위는 노드별로 누적 후 상한 클램프한다 - 여러 이웃과 겹친 허브가 한 프레임에
  // 멀리 튀는 것을 막는다(직접 이동 대신).
  const cdx = new Array(N).fill(0), cdy = new Array(N).fill(0);

  for (let i = 0; i < N; i++) for (let j = i + 1; j < N; j++) {
    const a = nodes[i], b = nodes[j];
    let dx = b.x - a.x, dy = b.y - a.y, d = Math.hypot(dx, dy);
    if (d < 0.5) {
      // (거의) 겹친 좌표는 방향이 0 이라 못 밀어낸다 - 결정적 방향으로 분리(퇴화 방지).
      const ang = ((i * 7 + j * 13) % 628) / 100;
      dx = Math.cos(ang); dy = Math.sin(ang); d = 0.5;
    } else { dx /= d; dy /= d; }
    const d2 = d * d;
    // 반발은 active(냉각 중)일 때만, 그리고 근거리 전용(사거리 밖은 0). 휴면 상태에서
    // 반발만 남아 응집(중력/스프링)과의 균형 없이 무한히 퍼지던 문제를 막는다.
    if (active && d < range) {
      const rf = repulse * alpha * (1 - d / range) / Math.max(d2, MIN_SEP * MIN_SEP);
      a.vx -= rf*dx; a.vy -= rf*dy; b.vx += rf*dx; b.vy += rf*dy;
    }
    const minD = nodeRadius(a) + nodeRadius(b) + COLLIDE_PAD;
    if (d < minD) {
      const push = (minD - d) / 2;
      cdx[i] -= dx*push; cdy[i] -= dy*push; cdx[j] += dx*push; cdy[j] += dy*push;
    }
  }

  // 잔여 겹침은 아래 충돌 변위가 프레임마다 밀어 정리한다(휴면 중에도 위치 보정은 적용).
  // 여기서 재가열하지 않는다 - 새 노드/드래그/리사이즈 같은 '이벤트'에서만 wake 로 깨운다
  // (정착 후 매 프레임 재가열이 도는 문제 제거).
  if (active) {
    for (const e of edges) {
      let dx = e.b.x - e.a.x, dy = e.b.y - e.a.y, d = Math.hypot(dx,dy) || 1;
      const f = (d - SPRING_LEN) * SPRING_K * alpha; dx /= d; dy /= d;
      e.a.vx += f*dx; e.a.vy += f*dy; e.b.vx -= f*dx; e.b.vy -= f*dy;
    }
  }

  const wcx = innerWidth/2, wcy = innerHeight/2;   // 월드 좌표(CSS 픽셀계) - 카메라와 독립
  // 그룹 모드: 타입별 목표점을 원형으로 배치해 그룹을 공간적으로 분리한다(결정적: 타입
  // 정렬 순서로 각도 배정). 그룹 목표 인력이 중심 인력을 대체하고, 브리지 엣지(스프링)가
  // 그룹 사이로 연결 노드를 당겨 "찾아갈 수 있는" 연결이 남는다.
  let tgt = null;
  if (clusterMode && active) {
    const types = Object.keys(typeColor).sort(), k = Math.max(1, types.length);
    const R = Math.min(innerWidth, innerHeight) * 0.34;
    tgt = {};
    types.forEach((t, i) => { const a = (i / k) * Math.PI * 2; tgt[t] = [wcx + R * Math.cos(a), wcy + R * Math.sin(a)]; });
  }
  for (let k = 0; k < N; k++) {
    const v = nodes[k];
    if (pinned(v)) { v.vx = 0; v.vy = 0; continue; }
    if (active) {
      if (tgt) {
        const g = tgt[v.type] || [wcx, wcy];
        v.vx += (g[0] - v.x) * CLUSTER_PULL * alpha; v.vy += (g[1] - v.y) * CLUSTER_PULL * alpha;
      } else {
        v.vx += (wcx - v.x) * centerG * alpha; v.vy += (wcy - v.y) * centerG * alpha;
      }
    }
    v.vx *= DAMPING; v.vy *= DAMPING;
    // 속도 상한 - 큰 힘이 걸려도 화면 밖으로 날아가지 않는다.
    const sp = Math.hypot(v.vx, v.vy);
    if (sp > maxV) { v.vx *= maxV/sp; v.vy *= maxV/sp; }
    // 충돌 변위도 노드별 상한으로 클램프해 더한다.
    let mx = cdx[k], my = cdy[k]; const m = Math.hypot(mx, my);
    if (m > MAX_PUSH) { mx *= MAX_PUSH/m; my *= MAX_PUSH/m; }
    v.x += v.vx + mx; v.y += v.vy + my;
  }
}

function draw() {
  stepSim();
  easeCam();
  // 초기 자동 맞춤: 레이아웃이 정착한 뒤 한 번(사용자 조작 전에만).
  if (needFit && alpha < ALPHA_MIN && !userMoved) { needFit = false; fitView(); }

  const act = activeSet();
  const anchor = focus || hover;
  ctx.setTransform(1,0,0,1,0,0);
  ctx.clearRect(0,0,canvas.width,canvas.height);

  // 엣지 + 노드는 world 변환(줌/팬, DPR 슈퍼샘플링 포함), 라벨은 화면 좌표(가독성 유지).
  ctx.setTransform(cam.s*DPR, 0, 0, cam.s*DPR, cam.x*DPR, cam.y*DPR);
  ctx.lineCap = "round"; ctx.lineJoin = "round";
  for (let i = 0; i < edges.length; i++) {
    const e = edges[i];
    if (typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
    const dx = e.b.x-e.a.x, dy = e.b.y-e.a.y, d = Math.hypot(dx,dy) || 1, ux = dx/d, uy = dy/d;
    const ar = nodeRadius(e.a), br = nodeRadius(e.b), room = d - ar - br;
    if (room <= 0.5) continue;   // (일시적) 겹침 - 이 프레임 엣지 생략
    const hot = act ? act.es.has(i) : false;
    // 선은 소스 노드 가장자리에서 시작해 화살촉 base 에서 끝난다 - 노드/화살표 관통 없음.
    const alen = Math.min(9/cam.s, Math.max(3/cam.s, room * 0.5));   // 화면상 ~9px, 가까우면 축소
    const sx0 = e.a.x + ux*ar, sy0 = e.a.y + uy*ar;
    const tipx = e.b.x - ux*br, tipy = e.b.y - uy*br;
    const basex = tipx - ux*alen, basey = tipy - uy*alen;
    // 그룹 모드: 그룹 간(다른 타입) 엣지는 도드라지게, 그룹 내 엣지는 흐리게 - 클러스터
    // 사이 연결성이 드러난다.
    const cross = clusterMode && e.a.type !== e.b.type;
    ctx.globalAlpha = act ? (hot ? 1 : 0.06) : (clusterMode ? (cross ? 0.9 : 0.1) : 0.85);
    ctx.strokeStyle = e.valid_to ? EDGE_OLD : (hot ? EDGE_HI : (cross ? "#c0caf5" : EDGE));
    ctx.lineWidth = (hot ? 2 : (cross ? 1.7 : 1.1)) / cam.s;   // 화면상 일정 두께
    ctx.setLineDash(e.valid_to ? [5/cam.s, 5/cam.s] : []);
    ctx.beginPath(); ctx.moveTo(sx0, sy0); ctx.lineTo(basex, basey); ctx.stroke();
    ctx.setLineDash([]);
    // 화살촉: base -> tip(도착 노드 가장자리). 선이 base 에서 끝나므로 삼각형과 겹치지 않는다.
    const hw = alen * 0.55;
    ctx.beginPath(); ctx.moveTo(tipx, tipy);
    ctx.lineTo(basex - uy*hw, basey + ux*hw);
    ctx.lineTo(basex + uy*hw, basey - ux*hw);
    ctx.closePath(); ctx.fillStyle = ctx.strokeStyle; ctx.fill();
  }
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const on = act ? act.ns.has(n.id) : true;
    ctx.globalAlpha = on ? 1 : 0.12;
    const r = nodeRadius(n);
    ctx.beginPath(); ctx.arc(n.x, n.y, r, 0, 7);
    ctx.fillStyle = typeColor[n.type] || OTHER; ctx.fill();
    if (n === anchor) { ctx.lineWidth = 2.5/cam.s; ctx.strokeStyle = INK; ctx.stroke(); }
    // 대화 발자국: 이 세션이 만진 노드는 지속적인 얇은 보라 링으로 표시.
    if (footprint.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 3.5, 0, 7);
      ctx.lineWidth = 1.5/cam.s; ctx.strokeStyle = "#9085e9"; ctx.stroke();
    }
    // 그룹 모드: 브리지 노드(다른 그룹과 연결)는 연한 링으로 표시 - 그룹 간 이동 지점.
    if (clusterMode && bridgeSet.has(n.id)) {
      ctx.beginPath(); ctx.arc(n.x, n.y, r + 2, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = "#c0caf5"; ctx.stroke();
    }
  }
  // 이벤트 펄스(에이전트가 만진 노드) - 확장하며 사라지는 링. rAF 는 항상 도므로 냉각
  // 후에도 애니메이션된다.
  for (const [id, ttl] of pulses) {
    const n = nodeById(id);
    if (!n || ttl <= 0 || typeOff.has(n.type)) { pulses.delete(id); continue; }
    const t = 1 - ttl/60;
    ctx.globalAlpha = (1 - t) * 0.85;
    ctx.beginPath(); ctx.arc(n.x, n.y, nodeRadius(n) + 3 + t*22, 0, 7);
    ctx.lineWidth = 2/cam.s; ctx.strokeStyle = EDGE_HI; ctx.stroke();
    pulses.set(id, ttl - 1);
  }
  ctx.globalAlpha = 1;

  // 라벨(화면 좌표, DPR 스케일만) - 확대됐거나 소규모면 전부, 아니면 강조/고차수 노드만.
  ctx.setTransform(DPR, 0, 0, DPR, 0, 0);
  ctx.font = "12px system-ui,-apple-system,sans-serif";
  ctx.textBaseline = "middle";
  // 라벨 정리: 소규모(<=40)이거나 충분히 확대(cam.s>1.4)면 전부, 큰 그래프에선 허브
  // (고차수 >= cut)만 + hover/focus/active. hairball 의 라벨 벽을 없앤다.
  const cut = (nodes.length <= 40 || cam.s > 1.4) ? 0 : Math.max(4, Math.round(nodes.length / 25));
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const on = act ? act.ns.has(n.id) : true;
    const show = on && (n === hover || n === focus || (act && act.ns.has(n.id)) || (n.degree || 0) >= cut);
    if (!show) continue;
    const px = n.x*cam.s + cam.x, py = n.y*cam.s + cam.y, r = nodeRadius(n)*cam.s;
    ctx.globalAlpha = on ? 1 : 0.25;
    ctx.lineWidth = 3; ctx.strokeStyle = SURFACE; ctx.strokeText(n.name, px + r + 5, py);
    ctx.fillStyle = (n === focus || n === hover) ? INK : INK2; ctx.fillText(n.name, px + r + 5, py);
  }
  ctx.globalAlpha = 1;
  requestAnimationFrame(draw);
}

function nodeAt(sx, sy) {
  const [wx, wy] = toWorld(sx, sy);
  let best = null, bd = 1e9;
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const d = Math.hypot(n.x - wx, n.y - wy);
    if (d < nodeRadius(n) + 6 && d < bd) { bd = d; best = n; }
  }
  return best;
}

function showTip(n, cx, cy) {
  if (!n) { tip.style.display = "none"; return; }
  tip.style.display = "block";
  tip.style.left = Math.min(cx + 14, innerWidth - 330) + "px";
  tip.style.top = (cy + 14) + "px";
  tip.innerHTML = `<b>${n.name}</b><br>`
    + `<span class="k">type</span> ${n.type} &nbsp; <span class="k">degree</span> ${n.degree || 0}<br>`
    + `<span class="k">sources</span> ${n.sources} &nbsp; <span class="k">trust</span> ${n.trust_tier}`;
}

// --- 상호작용 ---------------------------------------------------------------------
canvas.addEventListener("wheel", ev => {
  ev.preventDefault();
  zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);
}, { passive: false });

canvas.addEventListener("mousedown", ev => {
  downPos = { x: ev.clientX, y: ev.clientY };
  const n = nodeAt(ev.clientX, ev.clientY);
  if (n) { drag = n; wake(0.3); }
  else { panning = { sx: ev.clientX, sy: ev.clientY, px: cam.x, py: cam.y }; canvas.classList.add("grabbing"); }
});
addEventListener("mousemove", ev => {
  if (drag) { const [wx, wy] = toWorld(ev.clientX, ev.clientY); drag.x = wx; drag.y = wy; showTip(null); return; }
  if (panning) {
    // 팬은 즉시(1:1) - cam 과 camT 를 함께 옮겨 이징이 끌어당기지 않게 한다.
    cam.x = camT.x = panning.px + (ev.clientX - panning.sx);
    cam.y = camT.y = panning.py + (ev.clientY - panning.sy);
    userMoved = true; showTip(null); return;
  }
  hover = nodeAt(ev.clientX, ev.clientY);
  showTip(hover, ev.clientX, ev.clientY);
});
addEventListener("mouseup", ev => {
  const moved = downPos && Math.hypot(ev.clientX - downPos.x, ev.clientY - downPos.y) > 4;
  if (!moved && ev.target === canvas) {
    const n = nodeAt(ev.clientX, ev.clientY);
    focus = n ? (focus === n ? null : n) : null;   // 노드 클릭=포커스 토글(핀), 빈곳=해제
    if (focus) { wake(0.3); centerOn(focus); }    // 포커스-투-줌: 해당 노드로 부드럽게 센터
    renderDetail(focus);                           // 상세 인스펙터 표시/해제
  }
  if (drag) wake(0.3);
  drag = null;
  if (panning) { panning = null; canvas.classList.remove("grabbing"); }
  downPos = null;
});

searchEl.addEventListener("input", () => { searchTerm = searchEl.value.trim().toLowerCase(); });
searchEl.addEventListener("keydown", ev => {
  if (ev.key === "Enter" && searchTerm) {
    const hits = nodes.filter(n => n.name.toLowerCase().includes(searchTerm));
    if (hits.length) { fitView(hits, 140); userMoved = true; }
  }
});
document.getElementById("reload").onclick = () => { loadWorkspaces(); poll(); };
wsInput.addEventListener("keydown", e => { if (e.key === "Enter") { renderChipsActive(); poll(); } });
document.getElementById("zin").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1.2);
document.getElementById("zout").onclick = () => zoomAt(innerWidth/2, innerHeight/2, 1/1.2);
document.getElementById("fit").onclick = () => { userMoved = true; fitView(); };
const followBtn = document.getElementById("followBtn");
followBtn.onclick = () => { follow = !follow; followBtn.classList.toggle("on", follow); };
const clusterBtn = document.getElementById("clusterBtn");
clusterBtn.onclick = () => { clusterMode = !clusterMode; clusterBtn.classList.toggle("on", clusterMode); wake(0.6); poll(); };

resize(); loadWorkspaces(); poll(); connectEvents();
setInterval(poll, 2500); setInterval(loadWorkspaces, 5000); draw();
</script>
"###;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_addr_accepts_loopback_rejects_public() {
        assert!(parse_local_addr("127.0.0.1:7373").is_ok());
        assert!(parse_local_addr("127.0.0.1:0").is_ok());
        assert!(parse_local_addr("[::1]:7373").is_ok());
        // 비로opback 바인드는 거부(원칙 17).
        assert!(parse_local_addr("0.0.0.0:7373").is_err());
        assert!(parse_local_addr("192.168.1.10:7373").is_err());
        // 형식 오류.
        assert!(parse_local_addr("localhost:7373").is_err());
        assert!(parse_local_addr("nonsense").is_err());
    }

    #[test]
    fn percent_decode_basics() {
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("a%20b"), "a b");
        // 잘못된 시퀀스는 원문 유지.
        assert_eq!(percent_decode("a%zz"), "a%zz");
    }
}
