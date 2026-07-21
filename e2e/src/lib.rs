//! supragnosis-e2e - real-model end-to-end measurement suite.
//!
//! This crate is not a regression guard but a **measurement tool**: it produces a scorecard of
//! how well real LLMs (local Ollama, Anthropic API) use the supragnosis MCP surface, and of how
//! much knowledge delegation improves recall/ingestion/work output. Every test is `#[ignore]` +
//! externally dependent (model servers) + nondeterministic, so it does not run under the default
//! `cargo test`, and it silently skips when the external dependency is absent.
//!
//! Deterministic contract tests for the shipped artifacts (for example supragnosis-mcp's
//! mcp_surface) do not belong here - those tests are best left in their own crate. The
//! deterministic regression eval set (recall_eval) likewise lives in supragnosis-engine.
//!
//! Suite layout (tests/):
//!   - ollama_eval          : small-model MCP tool-use accuracy (single-turn + agent loop)
//!   - llm_eval             : same as above, but the Anthropic Messages API edition
//!   - delegation_eval      : knowledge-delegation gain A/B (recall QA, token cost/benefit, abstention/hallucination)
//!   - ontology_build_eval  : ontology ingestion quality as a by-product of work
//!   - physics_coding_eval  : whether a shared-ontology delegation shows up in coding output (+ behavior scoring)
//!
//! Shared harness:
//!   - [`bridge`] : in-process MCP server startup + Ollama (OpenAI-compatible) tool-calling bridge
//!   - [`report`] : report/gallery output paths (target/eval-reports/)

pub mod bridge;
pub mod embedders;
pub mod report;
