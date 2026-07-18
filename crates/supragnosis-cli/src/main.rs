//! supragnosis 실행 바이너리.
//!
//! stdio MCP 서버. 저장소는 파일 기반 Cozo(RocksDB, 기본) 또는 in-memory 선택.
//! 임베딩은 fastembed(feature) / hashing / none 중 선택 - 없으면 키워드 검색으로 degrade.
//! (`--http` 원격 노출, `--server` 허브, sync는 이후 마일스톤에서 추가.)

use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::{transport::stdio, ServiceExt};
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

    let engine = Engine::new(store, host.clone(), workspace.clone());
    let engine = match embedder {
        Some(embedder) => engine.with_embedder(embedder),
        None => engine,
    };
    let engine = Arc::new(engine);
    let server = SupragnosisServer::new(engine);

    tracing::info!(%host, %workspace, "supragnosis / stdio MCP 서버 시작");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
