//! supragnosis 실행 바이너리.
//!
//! M1: stdio MCP 서버. 저장소는 파일 기반 Cozo(RocksDB, 기본) 또는 in-memory 선택.
//! (`--http` 원격 노출, `--server` 허브, sync는 이후 마일스톤에서 추가.)

use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::{transport::stdio, ServiceExt};
use supragnosis_core::KnowledgeStore;
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::{CozoStore, InMemoryStore};

fn default_data_dir() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{home}/.supragnosis/db")
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

    let store: Arc<dyn KnowledgeStore> = match store_kind.as_str() {
        "mem" | "memory" => {
            tracing::info!("store=in-memory (비영속)");
            Arc::new(InMemoryStore::new())
        }
        _ => {
            let data_dir =
                std::env::var("SUPRAGNOSIS_DATA_DIR").unwrap_or_else(|_| default_data_dir());
            let store = CozoStore::open(&data_dir)
                .with_context(|| format!("failed to open Cozo store at {data_dir}"))?;
            tracing::info!(data_dir, "store=cozo (RocksDB, 영속)");
            Arc::new(store)
        }
    };

    let engine = Arc::new(Engine::new(store, host.clone(), workspace.clone()));
    let server = SupragnosisServer::new(engine);

    tracing::info!(%host, %workspace, "supragnosis M1 / stdio MCP 서버 시작");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
