//! supragnosis 실행 바이너리.
//!
//! MCP 서버. 저장소는 파일 기반 Cozo(RocksDB, 기본) 또는 in-memory 선택.
//! 임베딩은 fastembed(feature) / hashing / none 중 선택 - 없으면 키워드 검색으로 degrade.
//!
//! 전송 두 가지:
//! - 기본: **stdio** (Claude Code 등이 chat 마다 자식 프로세스로 기동).
//! - `SUPRAGNOSIS_HTTP_ADDR`(예 127.0.0.1:7373): **standalone 데몬** - MCP streamable-http 를
//!   상시 노출한다. 에이전트는 `claude mcp add --transport http http://127.0.0.1:7373/mcp` 로
//!   접속(chat 스폰 없이). 데몬이 db 의 유일한 보유자라 단일 프로세스 lock 문제도 해소.
//!   loopback 전용 바인드(원칙 17: 로컬 신뢰 표면, 무인증). 비로컬/인증은 후속.
//!
//! `SUPRAGNOSIS_VIZ_ADDR`(예 127.0.0.1:7374): 온톨로지 라이브 뷰어를 함께 띄운다(두 모드 공통).
//! (`--server` 허브, sync 는 이후 마일스톤.)

use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{stdio, StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServiceExt;
use supragnosis_core::{EmbeddingProvider, KnowledgeStore};
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::{CozoStore, InMemoryStore};

fn default_data_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{home}/.supragnosis/db")
}

/// 컴파일된 기능에 따른 기본 임베더 종류. fastembed 를 켜서 빌드했으면 그것이 기본.
fn default_embed_kind() -> &'static str {
    if cfg!(feature = "fastembed") {
        "fastembed"
    } else {
        "none"
    }
}

/// SUPRAGNOSIS_EMBED 값으로 임베딩 공급자를 고른다. 실패/부재는 None(키워드 검색으로 degrade).
fn build_embedder(kind: &str) -> Option<Arc<dyn EmbeddingProvider>> {
    match kind {
        "none" | "" => None,
        // 결정적이지만 비의미(어휘 해싱) - 개발/오프라인 스탠드인.
        "hashing" => {
            tracing::info!("embed=hashing (결정적/어휘 기반, 개발용)");
            Some(Arc::new(HashingEmbedder::default()))
        }
        "fastembed" => build_fastembed(),
        other => {
            tracing::warn!(
                kind = other,
                "알 수 없는 SUPRAGNOSIS_EMBED - 키워드 검색으로 진행"
            );
            None
        }
    }
}

#[cfg(feature = "fastembed")]
fn build_fastembed() -> Option<Arc<dyn EmbeddingProvider>> {
    match supragnosis_embed::FastEmbedProvider::try_default() {
        Ok(p) => {
            tracing::info!("embed=fastembed (BGE-small-en-v1.5, 384d)");
            Some(Arc::new(p))
        }
        Err(e) => {
            tracing::warn!(error = %e, "fastembed 초기화 실패 - 키워드 검색으로 진행");
            None
        }
    }
}

#[cfg(not(feature = "fastembed"))]
fn build_fastembed() -> Option<Arc<dyn EmbeddingProvider>> {
    tracing::warn!(
        "fastembed feature 미컴파일 - `--features fastembed` 로 빌드. 키워드 검색으로 진행"
    );
    None
}

/// 온톨로지 라이브 뷰어를 opt-in 으로 띄운다. `SUPRAGNOSIS_VIZ_ADDR`(IP:포트, loopback)이
/// 설정돼 있으면 이벤트 채널과 함께 localhost HTTP 서버를 spawn 한다. 설정/바인드 실패는
/// 로그만 남기고 MCP 서버 기동을 막지 않는다(뷰어는 보조 채널 - 원칙 21 의 도구 표면과 무관).
/// `events` 는 엔진에 붙은 싱크와 같은 broadcast 채널의 Sender - SSE 구독의 원천이다.
async fn spawn_viz(
    engine: &Arc<Engine>,
    addr_str: &str,
    events: tokio::sync::broadcast::Sender<String>,
) {
    let addr = match supragnosis_viz::parse_local_addr(addr_str) {
        Ok(addr) => addr,
        Err(e) => {
            tracing::error!(error = %e, "SUPRAGNOSIS_VIZ_ADDR 무시 - 뷰어 없이 진행");
            return;
        }
    };
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(error = %e, %addr, "viz 바인드 실패 - 뷰어 없이 진행");
            return;
        }
    };
    let bound = listener.local_addr().unwrap_or(addr);
    tracing::info!("온톨로지 뷰어 시작: http://{bound}");
    let engine = Arc::clone(engine);
    tokio::spawn(async move {
        if let Err(e) = supragnosis_viz::serve(engine, listener, events).await {
            tracing::error!(error = %e, "viz 서버 종료");
        }
    });
}

#[tokio::main]
async fn main() -> Result<()> {
    // 로그는 반드시 stderr 로. stdout 은 MCP stdio 트랜스포트 채널이다.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let host = std::env::var("SUPRAGNOSIS_HOST").unwrap_or_else(|_| "localhost".to_string());
    let workspace =
        std::env::var("SUPRAGNOSIS_WORKSPACE").unwrap_or_else(|_| "default".to_string());
    let store_kind = std::env::var("SUPRAGNOSIS_STORE").unwrap_or_else(|_| "cozo".to_string());

    // 임베더를 먼저 만든다 - Cozo 는 임베딩 차원으로 HNSW 인덱스를 세팅해야 하기 때문.
    let embed_kind =
        std::env::var("SUPRAGNOSIS_EMBED").unwrap_or_else(|_| default_embed_kind().to_string());
    let embedder = build_embedder(&embed_kind);
    let embed_dim = embedder.as_ref().map(|e| e.dimensions());

    let store: Arc<dyn KnowledgeStore> = match store_kind.as_str() {
        "mem" | "memory" => {
            tracing::info!("store=in-memory (비영속)");
            Arc::new(InMemoryStore::new())
        }
        _ => {
            let data_dir =
                std::env::var("SUPRAGNOSIS_DATA_DIR").unwrap_or_else(|_| default_data_dir());
            // 임베더 식별자(모델+차원)를 스토어에 기록/대조한다 - 다른 임베더로 재오픈하면
            // 침묵 오염(벡터 공간 혼합/부분 쓰기) 대신 여기서 명시적으로 실패한다.
            let store = match &embedder {
                Some(e) => CozoStore::open_with_embedder(&data_dir, &e.id(), e.dimensions()),
                None => CozoStore::open(&data_dir),
            }
            .with_context(|| format!("failed to open Cozo store at {data_dir}"))?;
            tracing::info!(data_dir, ?embed_dim, "store=cozo (RocksDB, 영속)");
            Arc::new(store)
        }
    };

    let mut engine = Engine::new(store, host.clone(), workspace.clone());
    // 세션 id (대화 발자국 그룹 키, 모든 이벤트에 실림). 우선순위:
    //   1) SUPRAGNOSIS_SESSION - 명시 override
    //   2) CLAUDE_CODE_SESSION_ID - Claude Code 가 자식 프로세스에 주입하는 세션 id(자동)
    //   3) host-<시작시각> - 그 외 실행의 서버 단위 폴백
    let session = std::env::var("SUPRAGNOSIS_SESSION")
        .or_else(|_| std::env::var("CLAUDE_CODE_SESSION_ID"))
        .unwrap_or_else(|_| format!("{host}-{}", supragnosis_core::now_millis()));
    engine = engine.with_session(session.clone());
    if let Some(embedder) = embedder {
        engine = engine.with_embedder(embedder);
    }

    // 뷰어(opt-in): 주소가 있으면 이벤트 채널을 만들고 엔진에 싱크를 붙인다 - MCP 도구
    // 호출이 이 채널로 활동을 발행하고, 뷰어의 /api/events(SSE)가 구독한다.
    let viz_addr = std::env::var("SUPRAGNOSIS_VIZ_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let events_tx =
        viz_addr.as_ref().map(|_| tokio::sync::broadcast::channel::<String>(256).0);
    if let Some(tx) = &events_tx {
        engine = engine.with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone())));
    }

    let engine = Arc::new(engine);

    // 뷰어 HTTP 서버 기동(같은 프로세스, localhost 전용 - 원칙 17). 실패해도 MCP 는 진행.
    if let (Some(addr_str), Some(tx)) = (viz_addr, events_tx) {
        spawn_viz(&engine, &addr_str, tx).await;
    }

    // standalone 데몬 모드: SUPRAGNOSIS_HTTP_ADDR 이 있으면 stdio 대신 MCP streamable-http 를
    // 상시 노출한다(뷰어는 위에서 이미 자기 포트로 기동됨).
    if let Ok(http_addr) = std::env::var("SUPRAGNOSIS_HTTP_ADDR") {
        if !http_addr.trim().is_empty() {
            return serve_http_daemon(engine, &http_addr, &host, &workspace, &session).await;
        }
    }

    let server = SupragnosisServer::new(engine);

    tracing::info!(%host, %workspace, %session, "supragnosis / stdio MCP 서버 시작");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// standalone 데몬: MCP streamable-http 서버를 상시 실행한다. 세션마다 factory 로
/// `SupragnosisServer` 를 만들되 `Arc<Engine>`(같은 db)을 공유한다. loopback 전용 바인드
/// (원칙 17: 로컬 신뢰 표면 - 무인증 정당). kill 로 종료(graceful shutdown 생략).
async fn serve_http_daemon(
    engine: Arc<Engine>,
    http_addr: &str,
    host: &str,
    workspace: &str,
    session: &str,
) -> Result<()> {
    let addr = supragnosis_viz::parse_local_addr(http_addr)?; // 비로컬 바인드 거부(원칙 17)
    let service = StreamableHttpService::new(
        move || Ok(SupragnosisServer::new(engine.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind MCP daemon at {addr}"))?;
    tracing::info!(%host, %workspace, %session, %addr, "supragnosis / MCP streamable-http 데몬: http://{addr}/mcp");
    axum::serve(listener, router).await?;
    Ok(())
}
