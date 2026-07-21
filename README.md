# supragnosis

여러 **호스트**와 **작업 공간**에서 발생하는 지식을 **온톨로지(개념/관계 그래프)** 로
구조화하고 **MCP** 로 질의/탐색하게 하는, 임베디드/파일 기반 Rust 서버.

> `supragnosis` = *supra*(위/너머) + *gnosis*(앎) - 지식 위의 지식(메타지식).

- 언어/런타임: **Rust** (`rmcp` 0.16 공식 MCP SDK, `tokio`)
- 저장소: **임베디드/파일 기반** `cozo`/RocksDB - 관계+그래프+벡터(HNSW) 통합.
- 상태: **M2 - 의미 검색 구현** (fastembed 의미+키워드 하이브리드 검색, Cozo 네이티브 HNSW, Cozo/RocksDB 영속, stdio MCP 서버 + standalone HTTP 데몬 + CLI 제어 + 라이브 뷰어). 설계 문서 -> [`docs/architecture.md`](docs/architecture.md), 설계 원칙 -> [`docs/principles.md`](docs/principles.md), 제안 워크플로 -> [`docs/proposal-workflow.md`](docs/proposal-workflow.md)

## 설치 (prebuilt 바이너리)
```bash
# 플랫폼 감지 -> 최신 릴리스 바이너리를 ~/.local/bin 에 설치(체크섬 검증)
curl -fsSL https://raw.githubusercontent.com/Ashon/supragnosis/main/scripts/install.sh | sh
```
- 또는 [Releases](https://github.com/Ashon/supragnosis/releases)에서 플랫폼 tar.gz 를 직접 내려받아 압축 해제 후 `supragnosis` 를 PATH 에 둔다.
- 지원 플랫폼: macOS(arm64/x86_64), Linux(x86_64). 다른 플랫폼은 아래 소스 빌드.
- prebuilt 는 **키워드 + hashing 검색**이다. 로컬 ONNX **의미 검색**은 소스에서 `--features fastembed` 로 빌드한다.
- 릴리스는 `v*` 태그 push 시 GitHub Actions(`.github/workflows/release.yml`)가 빌드/게시한다.

## 빌드 & 실행
```bash
cargo build                                          # 기본(키워드 검색) - 가벼운 빌드
cargo build -p supragnosis-cli --features fastembed  # 의미 검색(fastembed 로컬 ONNX 모델) 포함
cargo test                                           # 단위 테스트 (네트워크 필요한 fastembed 테스트는 --ignored 로 제외)
./target/debug/supragnosis                           # stdio MCP 서버 (MCP 클라이언트가 자식 프로세스로 기동)
```
- 환경변수:
  - `SUPRAGNOSIS_HOST` - 출처용 호스트 id (기본 `localhost`).
  - `SUPRAGNOSIS_WORKSPACE` - 기본 워크스페이스 (기본 `default`).
  - `SUPRAGNOSIS_STORE` - `cozo`(기본, 파일 영속) | `mem`(비영속).
  - `SUPRAGNOSIS_DATA_DIR` - Cozo 데이터 디렉터리 (기본 `~/.supragnosis/db`).
  - `SUPRAGNOSIS_EMBED` - `fastembed`(feature 컴파일 시 기본, 로컬 ONNX) | `hashing`(개발용) | `none`. 없거나 실패하면 키워드 검색으로 degrade.
- 도구: `observe`(지식 적재), `search_knowledge`(의미+키워드 하이브리드 검색), `get_entity`(엔티티+관계+출처 조회), `traverse`(관계 그래프 순회), `workspace_map`(공동출현 맥락/하이퍼엣지 개관).
- 크레이트: `core`(도메인/포트), `store`(어댑터), `engine`(서비스), `embed`(임베딩 어댑터), `mcp`(rmcp 도구), `viz`(라이브 뷰어), `cli`(바이너리).

## 사용 (CLI)
단일 바이너리를 서브커맨드로 제어한다. **인자 없이** 실행하면 stdio MCP 서버로 뜬다(MCP
클라이언트가 자식 프로세스로 기동하는 하위 호환 경로).
```bash
supragnosis                     # stdio MCP 서버 (기본, 무인자)
supragnosis serve --http 127.0.0.1:7373 --viz 127.0.0.1:7374   # 포그라운드 (HTTP 데몬 + 뷰어)
supragnosis start               # 백그라운드 데몬 시작 (기본 MCP :7373 + 뷰어 :7374)
supragnosis status              # 상태 (pid + 포트 헬스)
supragnosis stop                # 정지
supragnosis restart             # 재시작
supragnosis --help              # 전체 옵션
```
- 옵션 우선순위: 플래그 > `SUPRAGNOSIS_*` 환경변수 > 기본값.
- `start` 데몬은 자체 관리(launchd 불필요): pidfile `~/.supragnosis/supragnosis.pid` + 로그
  `~/.supragnosis/log`. 로그인 자동 기동 등 OS 서비스 등록은 [`deploy/README.md`](deploy/README.md).
- HTTP/뷰어는 loopback 전용(무인증 = 로컬 신뢰 표면). MCP 클라이언트 등록 예:
  - stdio: `claude mcp add supragnosis -- $(command -v supragnosis)`
  - HTTP(데몬): `claude mcp add supragnosis --transport http http://127.0.0.1:7373/mcp`

## 핵심 아이디어
- 지식은 **불변 관측(observation) 이벤트**로 들어오고(진실의 원천, 출처 보존),
  엔티티/관계 그래프는 로그로부터 **물질화**된다 (event sourcing).
- **로컬 우선 + 위상 독립 로그 복제** - 로컬 단독 / 중앙 서버(허브) / 피어 직접 / 하이브리드
  어느 연결 위상에서도 동일한 병합 의미론으로 충돌 없이 수렴.
- 헥사고날(포트-어댑터) 구조로 스토어/임베딩/추출기를 교체 가능하게 격리.

자세한 내용은 [아키텍처 설계 문서](docs/architecture.md)를 참고.
