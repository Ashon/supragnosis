//! supragnosis 실행 바이너리 (단일 바이너리 CLI).
//!
//! 서브커맨드로 서버를 제어한다. **인자 없이** 실행하면 stdio MCP 서버로 뜬다 - MCP
//! 클라이언트가 자식 프로세스로 기동하는 경로의 하위 호환이다.
//!
//!   supragnosis                  stdio MCP 서버 (기본, 무인자)
//!   supragnosis serve [옵션]      포그라운드 실행 (--http 주면 streamable-http 데몬, --viz 뷰어)
//!   supragnosis start [옵션]      백그라운드 데몬 시작 (기본 MCP 127.0.0.1:7373 + 뷰어 :7374)
//!   supragnosis stop             백그라운드 데몬 정지
//!   supragnosis restart [옵션]    정지 후 시작
//!   supragnosis status           데몬 상태
//!
//! 각 옵션은 대응 환경변수(SUPRAGNOSIS_*)를 폴백/기본값으로 쓴다(옵션이 우선). HTTP/뷰어는
//! loopback 전용(원칙 17). 백그라운드 데몬은 pidfile(~/.supragnosis/supragnosis.pid) +
//! 로그(~/.supragnosis/log)로 관리하는 자체 프로세스라 launchd 없이 동작한다(로그인 자동
//! 기동 등 OS 서비스 등록은 deploy/README.md).

use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::{stdio, StreamableHttpServerConfig, StreamableHttpService};
use rmcp::ServiceExt;
use supragnosis_core::{EmbeddingProvider, KnowledgeStore};
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::Engine;
use supragnosis_mcp::SupragnosisServer;
use supragnosis_store::{CozoStore, InMemoryStore};

#[derive(Parser)]
#[command(name = "supragnosis", version, about = "여러 호스트/워크스페이스의 지식을 온톨로지화하는 MCP 서버")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 포그라운드 실행 (기본 stdio, --http 주면 streamable-http 데몬)
    Serve(RunArgs),
    /// 백그라운드 데몬 시작 (기본 MCP :7373 + 뷰어 :7374)
    Start(RunArgs),
    /// 백그라운드 데몬 정지
    Stop,
    /// 데몬 재시작 (정지 후 시작)
    Restart(RunArgs),
    /// 데몬 상태 조회
    Status,
}

/// serve/start/restart 공통 실행 옵션. 미지정 시 SUPRAGNOSIS_* 환경변수 -> 기본값 순으로 해소.
#[derive(Args, Clone, Default)]
struct RunArgs {
    /// MCP streamable-http 바인드 주소(loopback). serve 는 생략 시 stdio, start 는 127.0.0.1:7373.
    #[arg(long, value_name = "ADDR")]
    http: Option<String>,
    /// 온톨로지 라이브 뷰어 바인드 주소(loopback). start 기본 127.0.0.1:7374.
    #[arg(long, value_name = "ADDR")]
    viz: Option<String>,
    /// 저장소: cozo(기본, 파일 영속) | mem(비영속).
    #[arg(long)]
    store: Option<String>,
    /// Cozo 데이터 디렉터리(기본 ~/.supragnosis/db).
    #[arg(long, value_name = "DIR")]
    data_dir: Option<String>,
    /// 출처용 호스트 id(기본 localhost).
    #[arg(long)]
    host: Option<String>,
    /// 기본 워크스페이스(기본 default).
    #[arg(long)]
    workspace: Option<String>,
    /// 임베더: fastembed | hashing | none.
    #[arg(long)]
    embed: Option<String>,
    /// 세션 id(발자국 그룹 키).
    #[arg(long)]
    session: Option<String>,
}

fn main() -> Result<()> {
    match Cli::parse().cmd.unwrap_or(Cmd::Serve(RunArgs::default())) {
        Cmd::Serve(a) => run_blocking(resolve(a, false)),
        Cmd::Start(a) => start(resolve(a, true)),
        Cmd::Stop => stop(),
        Cmd::Restart(a) => restart(resolve(a, true)),
        Cmd::Status => status(),
    }
}

/// 해소된 실행 설정.
struct Config {
    host: String,
    workspace: String,
    store_kind: String,
    data_dir: String,
    embed_kind: String,
    session: String,
    /// Some = streamable-http 데몬, None = stdio.
    http: Option<String>,
    /// Some = 라이브 뷰어 동반.
    viz: Option<String>,
}

/// RunArgs + 환경변수 + 기본값으로 Config 를 해소한다. `daemon=true`(start/restart)면 stdio 가
/// 무의미하므로 http/viz 를 loopback 기본값으로 채운다.
fn resolve(a: RunArgs, daemon: bool) -> Config {
    let env = |k: &str| std::env::var(k).ok().filter(|s| !s.trim().is_empty());
    let host = a
        .host
        .or_else(|| env("SUPRAGNOSIS_HOST"))
        .unwrap_or_else(|| "localhost".to_string());
    Config {
        workspace: a
            .workspace
            .or_else(|| env("SUPRAGNOSIS_WORKSPACE"))
            .unwrap_or_else(|| "default".to_string()),
        store_kind: a
            .store
            .or_else(|| env("SUPRAGNOSIS_STORE"))
            .unwrap_or_else(|| "cozo".to_string()),
        data_dir: a
            .data_dir
            .or_else(|| env("SUPRAGNOSIS_DATA_DIR"))
            .unwrap_or_else(default_data_dir),
        embed_kind: a
            .embed
            .or_else(|| env("SUPRAGNOSIS_EMBED"))
            .unwrap_or_else(|| default_embed_kind().to_string()),
        session: a
            .session
            .or_else(|| env("SUPRAGNOSIS_SESSION"))
            .or_else(|| env("CLAUDE_CODE_SESSION_ID"))
            .unwrap_or_else(|| format!("{host}-{}", supragnosis_core::now_millis())),
        http: a
            .http
            .or_else(|| env("SUPRAGNOSIS_HTTP_ADDR"))
            .or_else(|| daemon.then(|| "127.0.0.1:7373".to_string())),
        viz: a
            .viz
            .or_else(|| env("SUPRAGNOSIS_VIZ_ADDR"))
            .or_else(|| daemon.then(|| "127.0.0.1:7374".to_string())),
        host,
    }
}

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
            tracing::warn!(kind = other, "알 수 없는 SUPRAGNOSIS_EMBED - 키워드 검색으로 진행");
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
    tracing::warn!("fastembed feature 미컴파일 - `--features fastembed` 로 빌드. 키워드 검색으로 진행");
    None
}

/// 설정으로 스토어/임베더/엔진을 조립한다. `events` 가 있으면 UI 이벤트 싱크(뷰어)를 붙인다.
fn build_engine(
    cfg: &Config,
    events: Option<&tokio::sync::broadcast::Sender<String>>,
) -> Result<Arc<Engine>> {
    let embedder = build_embedder(&cfg.embed_kind);
    let embed_dim = embedder.as_ref().map(|e| e.dimensions());
    let store: Arc<dyn KnowledgeStore> = match cfg.store_kind.as_str() {
        "mem" | "memory" => {
            tracing::info!("store=in-memory (비영속)");
            Arc::new(InMemoryStore::new())
        }
        _ => {
            // 임베더 식별자(모델+차원)를 스토어에 기록/대조한다 - 다른 임베더로 재오픈 시 침묵
            // 오염 대신 명시적 실패.
            let store = match &embedder {
                Some(e) => CozoStore::open_with_embedder(&cfg.data_dir, &e.id(), e.dimensions()),
                None => CozoStore::open(&cfg.data_dir),
            }
            .with_context(|| format!("failed to open Cozo store at {}", cfg.data_dir))?;
            tracing::info!(data_dir = %cfg.data_dir, ?embed_dim, "store=cozo (RocksDB, 영속)");
            Arc::new(store)
        }
    };
    let mut engine =
        Engine::new(store, cfg.host.clone(), cfg.workspace.clone()).with_session(cfg.session.clone());
    if let Some(e) = embedder {
        engine = engine.with_embedder(e);
    }
    if let Some(tx) = events {
        engine = engine.with_events(Arc::new(supragnosis_viz::BroadcastSink::new(tx.clone())));
    }
    Ok(Arc::new(engine))
}

/// 실제 서버 실행(비동기). http 가 있으면 streamable-http 데몬, 없으면 stdio. viz 가 있으면
/// 같은 프로세스에서 라이브 뷰어를 함께 띄운다.
async fn run(cfg: Config) -> Result<()> {
    // 뷰어가 있을 때만 이벤트 채널을 만든다 - 엔진 싱크와 SSE 구독이 공유한다.
    let events = cfg
        .viz
        .as_ref()
        .map(|_| tokio::sync::broadcast::channel::<String>(256).0);
    let engine = build_engine(&cfg, events.as_ref())?;

    if let (Some(addr), Some(tx)) = (cfg.viz.as_ref(), events.as_ref()) {
        spawn_viz(&engine, addr, tx.clone()).await;
    }

    match cfg.http.as_deref() {
        Some(http) => serve_http_daemon(engine, http, &cfg.host, &cfg.workspace, &cfg.session).await,
        None => {
            tracing::info!(host = %cfg.host, workspace = %cfg.workspace, session = %cfg.session, "supragnosis / stdio MCP 서버 시작");
            let service = SupragnosisServer::new(engine).serve(stdio()).await?;
            service.waiting().await?;
            Ok(())
        }
    }
}

/// stderr 로그 구독자 초기화(멱등). stdout 은 MCP stdio 채널이므로 로그는 반드시 stderr.
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init();
}

/// tokio 런타임을 만들어 [`run`] 을 블로킹으로 돈다. `#[tokio::main]` 대신 수동 구축이라
/// start 의 데몬화(fork)가 런타임 생성 **전에** 일어날 수 있다(fork 후 런타임 파손 방지).
fn run_blocking(cfg: Config) -> Result<()> {
    init_tracing();
    let rt = tokio::runtime::Runtime::new().context("failed to build tokio runtime")?;
    rt.block_on(run(cfg))
}

/// 온톨로지 라이브 뷰어를 opt-in 으로 띄운다. 바인드/설정 실패는 로그만 남기고 서버 기동을
/// 막지 않는다(뷰어는 보조 채널 - 원칙 21). `events` 는 엔진 싱크와 같은 broadcast Sender.
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

/// standalone 데몬: MCP streamable-http 서버를 상시 실행한다. 세션마다 factory 로
/// `SupragnosisServer` 를 만들되 `Arc<Engine>`(같은 db)을 공유한다. loopback 전용 바인드
/// (원칙 17: 로컬 신뢰 표면 - 무인증 정당).
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

// --- 백그라운드 데몬 lifecycle (start/stop/restart/status) --------------------------
// pidfile + 로그 기반 자체 관리. kill(-0/SIGTERM)/TcpStream 만 쓰므로 unsafe/libc 불필요.

#[cfg(unix)]
fn base_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".supragnosis")
}
#[cfg(unix)]
fn pid_path() -> std::path::PathBuf {
    base_dir().join("supragnosis.pid")
}
#[cfg(unix)]
fn log_dir() -> std::path::PathBuf {
    base_dir().join("log")
}
#[cfg(unix)]
fn read_pid() -> Option<i32> {
    std::fs::read_to_string(pid_path())
        .ok()
        .and_then(|s| s.trim().parse().ok())
}
/// `kill -0` 로 프로세스 생존 확인(포터블, unsafe/libc 없이).
#[cfg(unix)]
fn pid_alive(pid: i32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
/// 주소에 리스너가 있는지(연결 시도 성공 = 사용 중).
#[cfg(unix)]
fn port_open(addr: &str) -> bool {
    use std::net::ToSocketAddrs;
    addr.to_socket_addrs()
        .ok()
        .and_then(|mut it| it.next())
        .map(|sa| {
            std::net::TcpStream::connect_timeout(&sa, std::time::Duration::from_millis(300)).is_ok()
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn start(cfg: Config) -> Result<()> {
    let http = cfg
        .http
        .clone()
        .unwrap_or_else(|| "127.0.0.1:7373".to_string());
    if let Some(pid) = read_pid() {
        if pid_alive(pid) {
            anyhow::bail!("이미 실행 중입니다 (pid {pid}). 'supragnosis stop' 후 다시 시도하세요.");
        }
    }
    if port_open(&http) {
        anyhow::bail!(
            "{http} 가 이미 사용 중입니다 (다른 인스턴스 또는 launchd 데몬?). 정지하거나 --http 로 다른 주소를 쓰세요."
        );
    }
    std::fs::create_dir_all(log_dir()).with_context(|| "로그 디렉터리 생성 실패")?;
    let out = std::fs::File::create(log_dir().join("supragnosis.out.log"))?;
    let err = std::fs::File::create(log_dir().join("supragnosis.err.log"))?;
    let viz_msg = cfg
        .viz
        .as_deref()
        .map(|v| format!("http://{v}"))
        .unwrap_or_else(|| "(off)".to_string());
    println!("supragnosis 데몬 시작 - MCP http://{http}/mcp  뷰어 {viz_msg}");
    println!("  pidfile {}  로그 {}", pid_path().display(), log_dir().display());
    // fork/setsid/pidfile/stdio 리다이렉트. 이후 코드는 데몬화된 자식에서만 실행된다.
    daemonize::Daemonize::new()
        .pid_file(pid_path())
        .stdout(out)
        .stderr(err)
        .start()
        .map_err(|e| anyhow::anyhow!("데몬화 실패: {e}"))?;
    run_blocking(cfg)
}

#[cfg(unix)]
fn stop() -> Result<()> {
    let Some(pid) = read_pid() else {
        println!("실행 중이 아닙니다 (pidfile 없음).");
        return Ok(());
    };
    if !pid_alive(pid) {
        let _ = std::fs::remove_file(pid_path());
        println!("실행 중이 아닙니다 (오래된 pidfile 정리, pid {pid}).");
        return Ok(());
    }
    std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()
        .with_context(|| "kill 실행 실패")?;
    // 종료 대기(최대 ~10s). SIGTERM 후 graceful 종료를 기다린다.
    for _ in 0..50 {
        if !pid_alive(pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if pid_alive(pid) {
        anyhow::bail!("정지 대기 초과 (pid {pid}). 수동 확인: kill {pid}");
    }
    let _ = std::fs::remove_file(pid_path());
    println!("데몬 정지 (pid {pid}).");
    Ok(())
}

#[cfg(unix)]
fn restart(cfg: Config) -> Result<()> {
    let _ = stop();
    std::thread::sleep(std::time::Duration::from_millis(400)); // 포트 해제 대기
    start(cfg)
}

#[cfg(unix)]
fn status() -> Result<()> {
    let http = std::env::var("SUPRAGNOSIS_HTTP_ADDR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1:7373".to_string());
    match read_pid() {
        Some(pid) if pid_alive(pid) => {
            println!("running (pid {pid})");
            println!(
                "  MCP http://{http}/mcp  ({})",
                if port_open(&http) { "응답" } else { "포트 무응답" }
            );
        }
        Some(pid) => println!("stopped (오래된 pidfile, pid {pid})"),
        None => println!("stopped"),
    }
    Ok(())
}

// 비-unix: 데몬 lifecycle 미지원 - serve --http 를 안내한다.
#[cfg(not(unix))]
fn start(_cfg: Config) -> Result<()> {
    anyhow::bail!("백그라운드 데몬(start)은 unix(macOS/Linux)에서만 지원합니다. 'supragnosis serve --http <ADDR>' 를 쓰세요.")
}
#[cfg(not(unix))]
fn stop() -> Result<()> {
    anyhow::bail!("백그라운드 데몬은 unix 전용입니다.")
}
#[cfg(not(unix))]
fn restart(_cfg: Config) -> Result<()> {
    anyhow::bail!("백그라운드 데몬은 unix 전용입니다.")
}
#[cfg(not(unix))]
fn status() -> Result<()> {
    anyhow::bail!("백그라운드 데몬은 unix 전용입니다.")
}
