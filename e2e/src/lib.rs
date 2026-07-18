//! supragnosis-e2e - 실모델 종단(end-to-end) 측정 스위트.
//!
//! 이 크레이트는 회귀 가드가 아니라 **측정 도구**다: 실제 LLM(로컬 Ollama, Anthropic API)이
//! supragnosis MCP 표면을 얼마나 잘 쓰는지, 지식 위임이 회상/적재/작업 산출물을 얼마나
//! 개선하는지 채점표(scorecard)를 낸다. 모든 테스트는 `#[ignore]` + 외부 의존(모델 서버)
//! + 비결정이므로 기본 `cargo test` 에서는 돌지 않고, 외부 의존이 없으면 조용히 skip 한다.
//!
//! 배포물의 결정적 계약 테스트(예: supragnosis-mcp 의 mcp_surface)는 여기 두지 않는다 -
//! 그런 테스트는 해당 크레이트에 남는 것이 맞다. 결정적 회귀 평가셋(recall_eval)도
//! 마찬가지로 supragnosis-engine 에 상주한다.
//!
//! 스위트 구성 (tests/):
//!   - ollama_eval          : 소형 모델의 MCP 도구 사용 정확도 (단일턴 + 에이전트 루프)
//!   - llm_eval             : 위와 같되 Anthropic Messages API 판
//!   - delegation_eval      : 지식 위임 이득 A/B (회수 QA, 토큰 손익, 부작위/환각)
//!   - ontology_build_eval  : 작업 부산물로서의 온톨로지 적재 품질
//!   - physics_coding_eval  : 공통 온톨로지 위임이 코딩 산출물에 반영되는가 (+ 행동 채점)
//!
//! 공용 하네스:
//!   - [`bridge`] : MCP 서버 in-process 구동 + Ollama(OpenAI 호환) tool-calling 브리지
//!   - [`report`] : 리포트/갤러리 출력 경로 (target/eval-reports/)

pub mod bridge;
pub mod embedders;
pub mod report;
