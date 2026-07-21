//! viz HTTP 표면 통합 테스트. 결정적 Engine(InMemory + 어휘 해싱)을 in-process 로
//! 조립하고, 포트 0 으로 바인드한 실제 리스너에 reqwest 로 GET 을 쏜다
//! (crates/supragnosis-mcp/tests/mcp_surface.rs 의 in-process 구동 관례를 따른다).

use std::sync::Arc;

use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::{Engine, EntityInput, Event, ObserveInput, RelationInput};
use supragnosis_store::InMemoryStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

/// 테스트용 이벤트 채널 - serve 에 넘길 broadcast Sender.
fn ev_channel() -> broadcast::Sender<String> {
    broadcast::channel::<String>(16).0
}

fn observe_depends(engine: &Engine) {
    engine
        .observe(ObserveInput {
            content: "supragnosis depends on rmcp".into(),
            workspace: None,
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput {
                    name: "supragnosis".into(),
                    kind: Some("Project".into()),
                },
                EntityInput {
                    name: "rmcp".into(),
                    kind: Some("Tool".into()),
                },
            ],
            relations: vec![RelationInput {
                from: "supragnosis".into(),
                kind: "depends_on".into(),
                to: "rmcp".into(),
                valid_from: None,
                valid_to: None,
            }],
        })
        .expect("observe 성공");
}

#[tokio::test]
async fn viz_serves_graph_index_and_404() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    observe_depends(&engine);

    // 포트 0 -> OS 할당 실제 포트 조회(결정적/충돌 없음).
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel()));

    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // /api/graph?workspace=ws -> 노드 2, 엣지 1.
    let resp = client
        .get(format!("{base}/api/graph?workspace=ws"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let g: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(g["stats"]["node_count"], 2, "graph: {g}");
    assert_eq!(g["stats"]["edge_count"], 1);
    assert_eq!(g["edges"][0]["type"], "depends_on");

    // 워크스페이스 미지정 -> 노드 기본 ws("ws") 스코프 -> 동일 2 노드.
    let g2: serde_json::Value = client
        .get(format!("{base}/api/graph"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(g2["stats"]["node_count"], 2);

    // '*' -> 전체(None) -> 동일.
    let g3: serde_json::Value = client
        .get(format!("{base}/api/graph?workspace=*"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(g3["stats"]["node_count"], 2);

    // 인덱스 HTML - canvas 뷰어.
    let idx = client.get(format!("{base}/")).send().await.unwrap();
    assert_eq!(idx.status(), 200);
    assert_eq!(
        idx.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let html = idx.text().await.unwrap();
    assert!(html.contains("<canvas"), "뷰어 HTML 에 canvas 가 있어야 한다");
    assert!(html.contains("/api/graph"), "뷰어가 그래프 API 를 폴링해야 한다");

    // 알 수 없는 경로 -> 404.
    let nf = client.get(format!("{base}/nope")).send().await.unwrap();
    assert_eq!(nf.status(), 404);
}

#[tokio::test]
async fn viz_lists_workspaces_sorted_distinct() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "alpha").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    // 두 워크스페이스에 지식 적재(도착 순서 뒤섞음).
    for (ws, name) in [("gamma", "x"), ("alpha", "y"), ("gamma", "z")] {
        engine
            .observe(ObserveInput {
                content: format!("{name} in {ws}"),
                workspace: Some(ws.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![EntityInput {
                    name: name.into(),
                    kind: None,
                }],
                relations: vec![],
            })
            .unwrap();
    }

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel()));

    let list: Vec<String> = reqwest::Client::new()
        .get(format!("http://{addr}/api/workspaces"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // 중복 제거 + 정렬(원칙 16).
    assert_eq!(list, vec!["alpha", "gamma"]);
}

/// SSE: 엔진 이벤트가 /api/events 로 스트리밍되는지 - 엔진에 BroadcastSink 를 붙이고
/// 같은 채널을 serve 에 준 뒤, 연결 -> emit -> data: 프레임 수신을 확인한다.
#[tokio::test]
async fn viz_streams_mcp_events_via_sse() {
    let tx = broadcast::channel::<String>(16).0;
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws")
            .with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone()))),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, tx.clone()));

    // SSE 연결 후 헤더를 먼저 읽는다(핸들러가 subscribe 를 마쳤다는 신호 - emit 순서 보장).
    let mut sock = TcpStream::connect(addr).await.unwrap();
    sock.write_all(b"GET /api/events HTTP/1.1\r\nHost: x\r\n\r\n")
        .await
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = sock.read(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(head.contains("text/event-stream"), "SSE content-type: {head}");

    // 이제 이벤트 발행 -> SSE data: 프레임으로 와야 한다.
    engine.emit(Event::GetEntity {
        id: "abc".into(),
        name: Some("rmcp".into()),
        found: true,
    });
    let mut got = String::new();
    for _ in 0..5 {
        let n = sock.read(&mut buf).await.unwrap();
        if n == 0 {
            break;
        }
        got.push_str(&String::from_utf8_lossy(&buf[..n]));
        if got.contains("data:") {
            break;
        }
    }
    assert!(
        got.contains("data:")
            && got.contains("get_entity")
            && got.contains("rmcp")
            && got.contains("\"session\""),
        "SSE 이벤트 프레임(세션 포함)이 와야 한다: {got}"
    );
}

/// `/api/hypergraph`: 한 관측이 공동 주장한 엔티티 집합이 하이퍼엣지로 나온다
/// (원칙 11 이차 구조). 라우팅 + 직렬화 + 엔진 배선을 HTTP 종단으로 가드한다.
#[tokio::test]
async fn viz_serves_hypergraph() {
    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(
        Engine::new(store, "h", "ws").with_embedder(Arc::new(HashingEmbedder::default())),
    );
    // 한 관측이 세 엔티티를 공동 주장 -> 하이퍼엣지 하나(size 3), 이진 관계 없음.
    engine
        .observe(ObserveInput {
            content: "supragnosis, rmcp, cozo were discussed together".into(),
            workspace: Some("ws".into()),
            source_ref: None,
            confidence: None,
            on_behalf_of: None,
            derived_from: vec![],
            entities: vec![
                EntityInput { name: "supragnosis".into(), kind: Some("Project".into()) },
                EntityInput { name: "rmcp".into(), kind: Some("Tool".into()) },
                EntityInput { name: "cozo".into(), kind: Some("Tool".into()) },
            ],
            relations: vec![],
        })
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(supragnosis_viz::serve(engine.clone(), listener, ev_channel()));

    let hg: serde_json::Value = reqwest::Client::new()
        .get(format!("http://{addr}/api/hypergraph?workspace=ws"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(hg["stats"]["node_count"], 3, "hypergraph: {hg}");
    assert_eq!(hg["stats"]["hyperedge_count"], 1);
    assert_eq!(hg["stats"]["max_size"], 3);
    assert_eq!(hg["hyperedges"][0]["size"], 3);
    // 멤버는 정렬된 엔티티 id 3개(결정적 - 원칙 16).
    assert_eq!(hg["hyperedges"][0]["members"].as_array().unwrap().len(), 3);
}
