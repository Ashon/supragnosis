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
        "/api/hypergraph" => hypergraph_response(engine, query),
        "/api/workspaces" => workspaces_response(engine),
        _ => Response {
            status: "404 Not Found",
            content_type: "application/json",
            body: err_body(
                "unknown path - try /, /api/graph, /api/hypergraph, /api/workspaces, or /api/events",
            ),
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

/// `/api/hypergraph` - 공동출현 이차 구조(하이퍼엣지) 프로젝션(원칙 11 이차 구조).
/// 워크스페이스 해석은 `/api/graph` 와 동일하다. 순수 읽기 파생 뷰이며(원칙 1), 뷰어가
/// hull 오버레이로 소비한다. 저장소 고장은 500 + 에러 바디(원칙 5: 고장은 빈 그래프가 아니다).
fn hypergraph_response(engine: &Engine, query: &str) -> Response {
    let ws_param = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("workspace="))
        .map(percent_decode);
    let ws_owned: Option<String> = match ws_param.as_deref() {
        None => Some(engine.default_workspace().to_string()),
        Some("") | Some("*") | Some("all") => None,
        Some(s) => Some(s.to_string()),
    };
    match engine.hypergraph(ws_owned.as_deref()) {
        Ok(hg) => match serde_json::to_string(&hg) {
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
                "note": "storage backend failure - NOT an empty hypergraph (원칙 5)"
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
  /* 토글 버튼 상태: 꺼짐=흐리게(dim, muted), 켜짐=accent 강조 - 상태가 한눈에 보인다.
     JS 는 .on 만 토글하고 .tog 는 유지한다. reload/zoom 같은 동작 버튼(.tog 없음)은 기본 그대로. */
  button.tog { opacity:.5; color:var(--muted); }
  button.tog:hover { opacity:.8; }
  button.tog.on { opacity:1; background:#2b3a52; border-color:var(--accent); color:var(--ink); }
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
  <button id="followBtn" class="tog on" title="follow agent activity: workspace + camera">follow</button>
  <button id="clusterBtn" class="tog" title="group by type; keep cross-group links visible">group</button>
  <button id="hyperBtn" class="tog" title="draw hyperedge hulls: co-occurrence sets (size>=3), 원칙 11 이차 구조">hulls</button>
  <button id="labelsBtn" class="tog on" title="toggle node/hull labels">labels</button>
  <button id="edgesBtn" class="tog on" title="toggle edges">edges</button>
  <button id="arrowsBtn" class="tog on" title="toggle edge direction arrowheads">arrows</button>
  <button id="footBtn" class="tog on" title="toggle session footprint rings">footprint</button>
  <button id="pulseBtn" class="tog on" title="toggle live activity pulses">pulses</button>
  <button id="histBtn" class="tog on" title="toggle superseded (past) edges">history</button>
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
// --- 카테고리 색 생성기(node/edge/hull 공용) ---------------------------------------
// hue 를 golden-angle(137.508deg)로 최대한 벌리고, 채도/명도를 단계별로 로테이션해 종류가
// 많아도(고정 팔레트 한계를 넘어) 서로 구별되게 한다. dark 배경 가독을 위해 명도 53~81% /
// 채도 62~92%. edge 는 라인이라 명도를 조금 높여 노드와 채널을 구분한다. 결정적 함수(같은
// index -> 같은 색). 참고: 카테고리가 아주 많아지면 인지적 구별력은 어떤 방법으로도 한계가 있다.
function catColor(i, edge) {
  const h = (i * 137.508) % 360;
  // 채도를 낮춰(약 50~74%) 라이트/다크 양쪽에서 튀지 않게, 명도는 중간대(양쪽 배경에서 대비
  // 확보)로. edge 는 라인이라 명도만 조금 높여 노드와 채널을 구분한다.
  const l = edge ? [70, 62, 73][i % 3] : [58, 66, 50][i % 3];
  const s = [62, 50, 74][(i / 3 | 0) % 3];
  return `hsl(${h | 0}, ${s}%, ${l}%)`;
}
const OTHER = "#898781", EDGE_OTHER = "#6b6b64";   // 맵에 없는 타입의 방어적 중립색
const EDGE = "#4a4a46", EDGE_HI = "#8fb4e6", EDGE_OLD = "#5a4a55";
const EDGE_ALPHA = 0.3;          // 엣지 기본 불투명도(낮게 - 밀집 그래프에서 뒤로 물러남). hover/focus 시 연결 엣지는 1.0 으로 활성화
// 노드 테두리는 마커 반경에 비례(zoom 시 마커와 함께 스케일 - 일관된 비율) + 화면 px 하한
// (줌아웃 시 사라지지 않게). 배경색 halo 라 노드/엣지/이웃을 분리한다(시인성).
const NODE_STROKE_RATIO = 0.35;  // 반경 대비 테두리 두께 비율(키움)
const NODE_STROKE_MIN = 2;       // 테두리 최소 두께(화면 px)
const INK = "#ffffff", INK2 = "#c3c2b7", SURFACE = "#1a1a19";

const canvas = document.getElementById("c"), ctx = canvas.getContext("2d");
const tip = document.getElementById("tip"), statusEl = document.getElementById("status");
const wsInput = document.getElementById("ws"), searchEl = document.getElementById("search");
const chipBar = document.getElementById("wschips"), legendEl = document.getElementById("legend");
const emptyEl = document.getElementById("empty"), logEl = document.getElementById("log");
const detailEl = document.getElementById("detail");

let follow = true;               // 카메라가 최신 에이전트 활동 노드를 따라가는지
let clusterMode = false;         // 타입 그룹으로 분리 배치 + 그룹 간 연결/브리지 강조
let hyperMode = false;           // 하이퍼엣지(공동출현 이차 구조) hull 오버레이 - 원칙 11
let hyperedges = [];             // [{id, members:[nodeId], size, sources, trust_tier}] - /api/hypergraph
// 그래픽 요소 visibility 토글(전부 기본 on). 레이아웃엔 영향 없는 순수 렌더 스위치.
let showLabels = true, showEdges = true, showArrows = true;
let showFootprint = true, showPulses = true, showSuperseded = true;
const bridgeSet = new Set();     // 다른 타입과 연결된 노드 id(그룹을 잇는 연결 노드)
const pulses = new Map();        // id -> 남은 프레임(이벤트 노드 강조 링 애니메이션)
const CLUSTER_PULL = 0.03;       // 그룹 목표점으로 끌어당기는 힘(중심 인력보다 강하게)
const HYPER_PULL = 0.03;         // 하이퍼엣지 무게중심 응집력(hull 안 노드를 타이트하게 모음)
const HULL_PAD = 24;             // hull 사이 목표 여백(월드 px). 밀도 높을 때 과분리 방지로 작게
const HULL_SEP = 0.008;          // hull 간 분리력(완만히 - 냉각 alpha 스케일)
const HULL_R_CAP = 160;          // 분리에 쓰는 hull 반경 상한 - 거대 grab-bag 이 전체를 밀지 못하게
const HULL_MAX_PUSH = 4;         // hull 당 프레임 분리 변위 상한 - 다수 쌍 누적 발산 방지
let footprintSession = null;     // 현재 발자국이 속한 세션(대화)
const footprint = new Set();     // 이 세션이 만진 노드 id들 - 대화의 지식 발자국
let nodes = [], edges = [], typeColor = {}, edgeTypeColor = {};
const posById = new Map();       // id -> {x,y,vx,vy} - 폴링 간 레이아웃 안정
const typeOff = new Set();       // 범례에서 숨긴 노드 타입
const edgeTypeOff = new Set();   // 범례에서 숨긴 엣지 종류(관계 kind)
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
// 노드 테두리 두께(월드 단위): 반경 비례 + 화면 px 하한. cam.s(현재 줌)를 반영해 zoom 시
// 마커와 함께 스케일되면서도 줌아웃에서 최소 두께를 유지한다. draw 와 엣지 종단이 공유한다.
function nodeStrokeW(n) { return Math.max(nodeRadius(n) * NODE_STROKE_RATIO, NODE_STROKE_MIN / cam.s); }
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
  types.forEach((t, i) => { typeColor[t] = catColor(i, false); });
  // 엣지 종류(관계 kind)별 색 - 결정적(kind 정렬 순), edge 대역으로 생성.
  const ek = [...new Set(edges.map(e => e.type))].sort();
  edgeTypeColor = {};
  ek.forEach((t, i) => { edgeTypeColor[t] = catColor(i, true); });
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
  // 노드 타입 + 엣지 종류를 각각 범례로. 클릭하면 해당 종류의 가시성을 토글한다(off 집합).
  const addGroup = (label, keys, colorOf, offSet, isEdge) => {
    if (!keys.length) return;
    const lbl = document.createElement("span"); lbl.className = "lbl"; lbl.textContent = label;
    legendEl.appendChild(lbl);
    for (const t of keys) {
      const el = document.createElement("span");
      el.className = "lg" + (offSet.has(t) ? " off" : "");
      const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = colorOf(t);
      if (isEdge) { sw.style.height = "3px"; sw.style.borderRadius = "2px"; }  // 라인 느낌
      el.appendChild(sw); el.appendChild(document.createTextNode(t || "(none)"));
      el.title = "click to toggle visibility";
      el.onclick = () => { if (offSet.has(t)) offSet.delete(t); else offSet.add(t); renderLegend(); };
      legendEl.appendChild(el);
    }
  };
  addGroup("nodes:", Object.keys(typeColor).sort(), t => typeColor[t], typeOff, false);
  addGroup("edges:", Object.keys(edgeTypeColor).sort(), t => edgeTypeColor[t], edgeTypeOff, true);
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
    // 하이퍼엣지(이차 구조)는 hull 모드일 때만 가져온다 - 없으면 비운다. 보조 채널이라
    // 실패해도 그래프 렌더는 유지한다(원칙 21: 관측가능성은 선택).
    if (hyperMode) {
      try {
        const hurl = "/api/hypergraph" + (ws ? "?workspace=" + encodeURIComponent(ws) : "");
        const hr = await fetch(hurl, { cache: "no-store" });
        if (hr.ok) {
          const hg = await hr.json();
          if (!hg.error) {
            hyperedges = hg.hyperedges || [];
            const drawn = hyperedges.filter(h => h.size >= 3).length;
            document.getElementById("stats").textContent += ` / hyperedges ${hyperedges.length} (hull ${drawn})`;
          }
        }
      } catch (e) { /* hull 은 보조 - 그래프는 그대로 */ }
    } else { hyperedges = []; }
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
  if (sEl) sEl.textContent = footprintSession ? `session ${footprintSession.slice(0,22)} / ${footprint.size} used` : "";
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

// 하이퍼엣지 id -> 팔레트 색(결정적 해시). 겹치는 hull 들이 반투명으로 블렌드된다(C1: 겹침=연결조직).
function hyperColor(id) {
  let h = 0; for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0;
  return catColor(h % 512, false);   // id 해시 -> 생성 팔레트 인덱스(고정 8색 한계 제거)
}
// 볼록 껍질(Andrew monotone chain). 결정적(입력 정렬) - 원칙 16. 3점 미만이면 그대로 반환.
function convexHull(pts) {
  if (pts.length < 3) return pts.slice();
  const p = pts.slice().sort((a, b) => a.x - b.x || a.y - b.y);
  const cross = (o, a, b) => (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
  const lo = [];
  for (const q of p) { while (lo.length >= 2 && cross(lo[lo.length-2], lo[lo.length-1], q) <= 0) lo.pop(); lo.push(q); }
  const up = [];
  for (let i = p.length - 1; i >= 0; i--) { const q = p[i]; while (up.length >= 2 && cross(up[up.length-2], up[up.length-1], q) <= 0) up.pop(); up.push(q); }
  lo.pop(); up.pop();
  return lo.concat(up);
}
// 두 하이퍼엣지가 노드를 공유하는가(작은 집합을 순회). 공유 = 연결된 맥락이라 분리하지 않는다(C1).
function hullsShareMember(a, b) {
  const [s, l] = a.size <= b.size ? [a, b] : [b, a];
  for (const x of s) if (l.has(x)) return true;
  return false;
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

  // 하이퍼엣지 레이아웃(원칙 11 이차 구조 "잘 응집"): (1) 각 하이퍼엣지의 무게중심으로
  // 멤버를 당겨 hull 을 타이트하게 응집시키고, (2) 겹치지 않는 hull 끼리는 무게중심을 밀어
  // 간격을 벌린다. 노드를 공유하는(겹치는) hull 은 공유 노드가 양쪽 중심에 동시에 당겨져
  // 자연히 붙어 있고, 분리 힘은 공유 노드에서 상쇄되어 겹침 관계가 보존된다(C1: 겹침=연결조직).
  if (hyperMode && active && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    // 지오메트리: 멤버 + 무게중심 + 평균 반경(상한 clamp - 거대 grab-bag 이 전체를 밀지 못하게)
    // + 멤버 id 집합(공유 판정용).
    const hgs = [];
    for (const h of hyperedges) {
      const ms = h.members.map(id => nb.get(id)).filter(Boolean);
      if (ms.length < 2) continue;
      let cx = 0, cy = 0; for (const m of ms) { cx += m.x; cy += m.y; }
      cx /= ms.length; cy /= ms.length;
      let r = 0; for (const m of ms) r += Math.hypot(m.x - cx, m.y - cy);
      r = Math.min(r / ms.length, HULL_R_CAP);
      hgs.push({ ms, ids: new Set(ms.map(m => m.id)), cx, cy, r });
    }
    // (1) 응집: 멤버 -> 자기 무게중심.
    for (const g of hgs) {
      for (const m of g.ms) {
        if (pinned(m)) continue;
        m.vx += (g.cx - m.x) * HYPER_PULL * alpha; m.vy += (g.cy - m.y) * HYPER_PULL * alpha;
      }
    }
    // (2) 분리: **노드를 공유하지 않는(disjoint)** hull 쌍만 밀어낸다 - 공유 hull 은 연결된
    // 맥락이라 붙어 있어야 하고(C1), 밀도 높을 때 모든 쌍을 밀면 레이아웃이 통째로 폭발한다.
    // 각 hull 의 순 변위를 누적해 상한(HULL_MAX_PUSH)으로 클램프한 뒤 멤버에 실어 발산을 막는다.
    const sepx = new Array(hgs.length).fill(0), sepy = new Array(hgs.length).fill(0);
    for (let i = 0; i < hgs.length; i++) for (let j = i + 1; j < hgs.length; j++) {
      const a = hgs[i], b = hgs[j];
      if (hullsShareMember(a.ids, b.ids)) continue;   // 연결된 맥락은 밀지 않는다(C1)
      const dx = b.cx - a.cx, dy = b.cy - a.cy, d = Math.hypot(dx, dy) || 0.01;
      const want = a.r + b.r + HULL_PAD;
      if (d < want) {
        const mag = (want - d) * HULL_SEP * alpha, ux = dx / d * mag, uy = dy / d * mag;
        sepx[i] -= ux; sepy[i] -= uy; sepx[j] += ux; sepy[j] += uy;
      }
    }
    for (let i = 0; i < hgs.length; i++) {
      let mx = sepx[i], my = sepy[i]; const mm = Math.hypot(mx, my);
      if (mm === 0) continue;
      if (mm > HULL_MAX_PUSH) { mx *= HULL_MAX_PUSH / mm; my *= HULL_MAX_PUSH / mm; }
      for (const m of hgs[i].ms) if (!pinned(m)) { m.vx += mx; m.vy += my; }
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
  // 하이퍼엣지 hull 오버레이(엣지/노드 뒤에 깔린다). size>=3 만 그린다 - 2 는 이진 엣지에
  // 수렴해 밀도 완화에 기여하지 않는다. 반투명이라 겹치는 맥락들이 블렌드된다(C1: 겹침=연결조직).
  // hover 한 노드가 속한 hull 은 강조하고, 대표 개념(최고 degree 멤버)+크기 라벨을 모은다.
  const hullLabels = [];
  if (hyperMode && hyperedges.length) {
    const nb = new Map(nodes.map(n => [n.id, n]));
    const PAD = 18;
    for (const h of hyperedges) {
      const ms = h.members.map(id => nb.get(id)).filter(m => m && !typeOff.has(m.type));
      if (ms.length < 3) continue;
      const hull = convexHull(ms.map(m => ({ x: m.x, y: m.y })));
      if (hull.length < 3) continue;
      let cx = 0, cy = 0; for (const q of hull) { cx += q.x; cy += q.y; } cx /= hull.length; cy /= hull.length;
      const col = hyperColor(h.id);
      const hot = hover && h.members.includes(hover.id);   // hover 노드가 이 맥락에 속하나
      ctx.beginPath();
      hull.forEach((q, i) => {
        const dx = q.x - cx, dy = q.y - cy, d = Math.hypot(dx, dy) || 1;
        const px = q.x + dx/d*PAD, py = q.y + dy/d*PAD;
        if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
      });
      ctx.closePath();
      // stroke 없이 fill 로만(부드러운 영역). 아웃라인이 사라진 만큼 base alpha 를 약간 올려
      // 개별 hull 가독을 지키고, 겹치는 영역은 alpha 누적으로 자연히 진해진다(밀도 단서, C1).
      ctx.globalAlpha = hot ? 0.26 : 0.12; ctx.fillStyle = col; ctx.fill();
      // 대표 개념 = 최고 degree 멤버(허브). 라벨은 화면 좌표로 나중에 그린다(크기 일정).
      let anchor = ms[0]; for (const m of ms) if ((m.degree||0) > (anchor.degree||0)) anchor = m;
      hullLabels.push({ cx, cy, text: anchor.name + " (" + ms.length + ")", col, hot });
    }
    ctx.globalAlpha = 1;
  }
  if (showEdges) for (let i = 0; i < edges.length; i++) {
    const e = edges[i];
    if (typeOff.has(e.a.type) || typeOff.has(e.b.type)) continue;
    if (edgeTypeOff.has(e.type)) continue;                 // 엣지 종류 토글(범례)
    if (e.valid_to && !showSuperseded) continue;           // 대체된(과거) 엣지 숨김(history 토글)
    const dx = e.b.x-e.a.x, dy = e.b.y-e.a.y, d = Math.hypot(dx,dy) || 1, ux = dx/d, uy = dy/d;
    // 노드 테두리 바깥에서 만나도록 반경을 각 노드 테두리 절반만큼 키운다(테두리가 반경 비례라
    // 끝점마다 다르다) - 화살촉 tip 이 테두리 밖 경계에 닿아 gap/관통 없이 연결되고, zoom 해도 유지.
    const ar = nodeRadius(e.a) + nodeStrokeW(e.a)/2, br = nodeRadius(e.b) + nodeStrokeW(e.b)/2, room = d - ar - br;
    if (room <= 0.5) continue;   // (일시적) 겹침 - 이 프레임 엣지 생략
    const hot = act ? act.es.has(i) : false;
    // 선은 소스 노드 가장자리에서 시작해 화살촉 base(또는 화살표 끄면 tip)에서 끝난다.
    const alen = Math.min(9/cam.s, Math.max(3/cam.s, room * 0.5));   // 화면상 ~9px, 가까우면 축소
    const sx0 = e.a.x + ux*ar, sy0 = e.a.y + uy*ar;
    const tipx = e.b.x - ux*br, tipy = e.b.y - uy*br;
    const basex = tipx - ux*alen, basey = tipy - uy*alen;
    // 그룹 모드: 그룹 간(다른 타입) 엣지는 도드라지게, 그룹 내 엣지는 흐리게.
    const cross = clusterMode && e.a.type !== e.b.type;
    // 기본은 반투명(EDGE_ALPHA), hover/focus 시 연결 엣지(hot)는 1.0 으로 활성화하고 나머지는
    // 흐리게. 그룹 모드는 그룹 간 연결을 도드라지게 하는 별도 강조.
    ctx.globalAlpha = act ? (hot ? 1 : 0.06) : (clusterMode ? (cross ? 0.9 : 0.1) : EDGE_ALPHA);
    // 색은 관계 종류(kind)로 - 어떤 연결인지 드러난다. 대체된 엣지는 EDGE_OLD(과거 신호, 점선).
    ctx.strokeStyle = e.valid_to ? EDGE_OLD : (edgeTypeColor[e.type] || EDGE_OTHER);
    ctx.lineWidth = (hot ? 2 : (cross ? 1.7 : 1.1)) / cam.s;   // 화면상 일정 두께
    ctx.setLineDash(e.valid_to ? [5/cam.s, 5/cam.s] : []);
    // 화살표 끄면 선을 노드 가장자리(tip)까지, 켜면 화살촉 base 까지.
    const endx = showArrows ? basex : tipx, endy = showArrows ? basey : tipy;
    ctx.beginPath(); ctx.moveTo(sx0, sy0); ctx.lineTo(endx, endy); ctx.stroke();
    ctx.setLineDash([]);
    if (showArrows) {
      // 화살촉: base -> tip(도착 노드 가장자리). 선이 base 에서 끝나 삼각형과 겹치지 않는다.
      const hw = alen * 0.55;
      ctx.beginPath(); ctx.moveTo(tipx, tipy);
      ctx.lineTo(basex - uy*hw, basey + ux*hw);
      ctx.lineTo(basex + uy*hw, basey - ux*hw);
      ctx.closePath(); ctx.fillStyle = ctx.strokeStyle; ctx.fill();
    }
  }
  for (const n of nodes) {
    if (typeOff.has(n.type)) continue;
    const on = act ? act.ns.has(n.id) : true;
    ctx.globalAlpha = on ? 1 : 0.12;
    const r = nodeRadius(n);
    ctx.beginPath(); ctx.arc(n.x, n.y, r, 0, 7);
    ctx.fillStyle = typeColor[n.type] || OTHER; ctx.fill();
    // 기본 테두리(배경색 halo): 노드 경계를 또렷하게 하고 엣지/이웃과 분리한다(시인성).
    // 배경색이라 테마가 바뀌어도 항상 배경과 맞는 컷아웃이 된다.
    ctx.lineWidth = nodeStrokeW(n); ctx.strokeStyle = SURFACE; ctx.stroke();
    if (n === anchor) { ctx.lineWidth = 2.5/cam.s; ctx.strokeStyle = INK; ctx.stroke(); }
    // 대화 발자국: 이 세션이 만진 노드는 지속적인 얇은 보라 링으로 표시(footprint 토글).
    if (showFootprint && footprint.has(n.id)) {
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
    if (showPulses) {
      const t = 1 - ttl/60;
      ctx.globalAlpha = (1 - t) * 0.85;
      ctx.beginPath(); ctx.arc(n.x, n.y, nodeRadius(n) + 3 + t*22, 0, 7);
      ctx.lineWidth = 2/cam.s; ctx.strokeStyle = EDGE_HI; ctx.stroke();
    }
    pulses.set(id, ttl - 1);   // 숨겨도 만료는 진행(토글 시 잔상 방지)
  }
  ctx.globalAlpha = 1;

  // 라벨(노드 + hull) - labels 토글로 켜고 끈다. 화면 좌표(DPR)라 줌 무관 일정 크기.
  if (showLabels) {
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
    // 하이퍼엣지 hull 라벨(대표 개념 + 크기). hover 한 hull 은 진하게. textAlign 은 복원.
    if (hullLabels.length) {
      ctx.font = "11px system-ui,-apple-system,sans-serif";
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      for (const l of hullLabels) {
        const px = l.cx*cam.s + cam.x, py = l.cy*cam.s + cam.y;
        ctx.globalAlpha = l.hot ? 1 : 0.7;
        ctx.lineWidth = 3.5; ctx.strokeStyle = SURFACE; ctx.strokeText(l.text, px, py);
        ctx.fillStyle = l.col; ctx.fillText(l.text, px, py);
      }
      ctx.textAlign = "left"; ctx.globalAlpha = 1;
    }
  }
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
const hyperBtn = document.getElementById("hyperBtn");
hyperBtn.onclick = () => { hyperMode = !hyperMode; hyperBtn.classList.toggle("on", hyperMode); wake(0.6); poll(); };
// 순수 렌더 토글(레이아웃/데이터 변화 없음 - rAF 가 매 프레임 반영하므로 wake/poll 불필요).
document.getElementById("labelsBtn").onclick = e => { showLabels = !showLabels; e.currentTarget.classList.toggle("on", showLabels); };
document.getElementById("edgesBtn").onclick = e => { showEdges = !showEdges; e.currentTarget.classList.toggle("on", showEdges); };
document.getElementById("arrowsBtn").onclick = e => { showArrows = !showArrows; e.currentTarget.classList.toggle("on", showArrows); };
document.getElementById("footBtn").onclick = e => { showFootprint = !showFootprint; e.currentTarget.classList.toggle("on", showFootprint); };
document.getElementById("pulseBtn").onclick = e => { showPulses = !showPulses; e.currentTarget.classList.toggle("on", showPulses); };
document.getElementById("histBtn").onclick = e => { showSuperseded = !showSuperseded; e.currentTarget.classList.toggle("on", showSuperseded); };

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
