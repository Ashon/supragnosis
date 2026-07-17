//! supragnosis 실행 바이너리.
//!
//! M0: stdio MCP 서버(in-memory 스토어)만 기동한다.
//! (`--http` 원격 노출, `--server` 허브, sync는 이후 마일스톤에서 추가.)

use std::sync::Arc;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::InMemoryStore;

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

    let store = Arc::new(InMemoryStore::new());
    let engine = Arc::new(Engine::new(store, host.clone(), workspace.clone()));
    let server = SupragnosisServer::new(engine);

    tracing::info!(%host, %workspace, "supragnosis M0 · stdio MCP 서버 시작 (in-memory store)");

    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
