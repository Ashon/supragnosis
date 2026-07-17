# supragnosis

여러 **호스트**와 **작업 공간**에서 발생하는 지식을 **온톨로지(개념·관계 그래프)** 로
구조화하고 **MCP** 로 질의·탐색하게 하는, 임베디드/파일 기반 Rust 서버.

> `supragnosis` = *supra*(위/너머) + *gnosis*(앎) — 지식 위의 지식(메타지식).

- 언어/런타임: **Rust** (`rmcp` 0.16 공식 MCP SDK, `tokio`)
- 저장소: **임베디드/파일 기반** (M0는 in-memory, M1부터 `cozo`/RocksDB — 관계+그래프+벡터 통합)
- 상태: **M0 골격 구현** (stdio MCP 서버). 설계 문서 → [`docs/architecture.md`](docs/architecture.md)

## 빌드 & 실행 (M0)
```bash
cargo build          # 워크스페이스 전체 빌드
cargo test           # 엔진 로직 단위 테스트
./target/debug/supragnosis   # stdio MCP 서버 (MCP 클라이언트가 자식 프로세스로 기동)
```
- 환경변수: `SUPRAGNOSIS_HOST`(출처용 호스트 id), `SUPRAGNOSIS_WORKSPACE`(기본 워크스페이스).
- M0 도구: `observe`(지식 적재), `get_entity`(엔티티+관계+출처 조회), `search_knowledge`(키워드 검색).
- 크레이트: `core`(도메인·포트) · `store`(어댑터) · `engine`(서비스) · `mcp`(rmcp 도구) · `cli`(바이너리).

## 핵심 아이디어
- 지식은 **불변 관측(observation) 이벤트**로 들어오고(진실의 원천, 출처 보존),
  엔티티·관계 그래프는 로그로부터 **물질화**된다 (event sourcing).
- **로컬 우선 + 위상 독립 로그 복제** — 로컬 단독 / 중앙 서버(허브) / 피어 직접 / 하이브리드
  어느 연결 위상에서도 동일한 병합 의미론으로 충돌 없이 수렴.
- 헥사고날(포트-어댑터) 구조로 스토어·임베딩·추출기를 교체 가능하게 격리.

자세한 내용은 [아키텍처 설계 문서](docs/architecture.md)를 참고.
