//! Delegation eval: scores, quantitatively and qualitatively, "is knowledge delegation actually a gain".
//!
//! Thesis: when a small model delegates knowledge to supragnosis MCP, it can produce the same
//! (or better) accuracy with fewer tokens, without stuffing the entire history into context.
//! This harness measures that thesis directly, A/B:
//!
//!   - baseline  condition: inject the whole corpus (core facts + noise facts) into the prompt and ask.
//!   - delegated condition: preload the same corpus into the engine (deterministically -
//!     decoupled from the model's ingestion quality), and the model pulls only as much as it
//!     needs via MCP tools (search_knowledge, etc.) to answer.
//!
//! Question categories and quantitative metrics:
//!   - direct      : plain recall accuracy (whether the gold keyword appears).
//!   - supersede   : when a fact has been updated, does it answer with the latest value (stale
//!     answer rate - Principle 3/6: conflict is information, the model must reconcile the two
//!     observations).
//!   - unanswerable: does it answer "I don't know" to a question absent from the corpus
//!     (abstention/hallucination rate - the greatest risk of the delegation structure is making
//!     something up without searching).
//!   - token cost  : per-condition sum of prompt+generation tokens, and tokens-per-correct.
//!
//! Qualitative evaluation: the full transcript (question/tool calls/args/results/final answer/
//! verdict) is left as a markdown report -> target/eval-reports/delegation_eval.md (an artifact
//! for human review).
//!
//! Judging is deterministic keyword matching (quantitative reproducibility). Subtle wrong answers
//! in free-form prose are caught by the report's qualitative review - an LLM-judge would add
//! nondeterminism, so it was intentionally left out.
//!
//! Nondeterministic (model) and requiring a local Ollama, so excluded from the default run.
//! Run:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test delegation_eval -- --ignored --nocapture
//! Optional env:
//!   OLLAMA_BASE_URL (default http://localhost:11434)
//!   OLLAMA_MODELS   (comma-separated, default gemma4)
//!   EVAL_RUNS       (repeat count, default 1 - use 3+ to measure small-model jitter as a pass-rate)
//!   EVAL_EMBEDDERS  (comma-separated, default hashing - e.g. hashing,fastembed. A/Bs delegated
//!                    recall per embedder. fastembed needs a --features real-embed build.
//!                    Isolates whether the token-collision gap of lexical hashing closes with a
//!                    semantic embedder)
//!   EVAL_SCALES     (comma-separated noise-fact counts, default 60 - e.g. 60,300,1000. An axis
//!                    to show, as a break-even curve, that baseline grows linearly more expensive
//!                    with the corpus and collapses once it exceeds the context window, whereas
//!                    delegated stays constant)
//!
//! If Ollama is not up, it silently passes (skips) - so as not to break CI.

use std::sync::Arc;
use std::time::Duration;

use rmcp::service::RunningService;
use rmcp::RoleClient;
use serde_json::{json, Value};

use supragnosis_e2e::bridge::{
    chat, exec_tool, ollama_reachable, openai_tools, push_message, serve_engine, tool_calls,
    DEFAULT_BASE,
};
use supragnosis_e2e::report;
use supragnosis_engine::{Engine, EntityInput, ObserveInput, RelationInput};
use supragnosis_store::InMemoryStore;

const DEFAULT_MODELS: &str = "gemma4";
const WS: &str = "eval";
/// Max tool round-trips in the delegated condition (infinite-loop guard).
const MAX_ROUNDS: usize = 5;

// epoch-millis constants for the validity-interval fixture.
const TS_2024_01: u64 = 1_704_067_200_000; // 2024-01-01
const TS_2025_03: u64 = 1_740_787_200_000; // 2025-03-01
const TS_2025_06: u64 = 1_748_736_000_000; // 2025-06-01

// --- Fixtures: corpus and questions ------------------------------------------

/// Relation fixture: (from, kind, to, valid_from, valid_to).
type FactRelation = (&'static str, &'static str, &'static str, Option<u64>, Option<u64>);

/// A single core fact: observation body + enclosed entities/relations (with validity intervals).
struct Fact {
    content: &'static str,
    entities: &'static [(&'static str, &'static str)],
    relations: &'static [FactRelation],
}

/// Core facts. The supersede pairs (4-5, 6-7) state dates in the body so that, when both
/// observations are recalled, the model can pick the latest (the engine automatically closing
/// out refuted facts is M3 - for now it is the model's job).
fn core_facts() -> Vec<Fact> {
    vec![
        Fact {
            content: "the acme project uses cozodb as its embedded storage engine",
            entities: &[("acme project", "Project"), ("cozodb", "Tool")],
            relations: &[("acme project", "uses", "cozodb", None, None)],
        },
        Fact {
            content: "the ingest pipeline publishes events through the nats message broker",
            entities: &[("ingest pipeline", "Component"), ("nats", "Tool")],
            relations: &[("ingest pipeline", "uses", "nats", None, None)],
        },
        Fact {
            content: "the billing service depends on the auth service for token validation",
            entities: &[("billing service", "Component"), ("auth service", "Component")],
            relations: &[("billing service", "depends_on", "auth service", None, None)],
        },
        // supersede pair 1: deployment target migration.
        Fact {
            content: "from january 2024 until june 2025 the acme api was deployed on heroku",
            entities: &[("acme api", "Component"), ("heroku", "Tool")],
            relations: &[(
                "acme api",
                "deployed_on",
                "heroku",
                Some(TS_2024_01),
                Some(TS_2025_06),
            )],
        },
        Fact {
            content: "since june 2025 the acme api is deployed on fly.io, migrated off heroku",
            entities: &[("acme api", "Component"), ("fly.io", "Tool")],
            relations: &[("acme api", "deployed_on", "fly.io", Some(TS_2025_06), None)],
        },
        // supersede pair 2: session cache replacement.
        Fact {
            content: "until march 2025 the acme api cached sessions with redis",
            entities: &[("acme api", "Component"), ("redis", "Tool")],
            relations: &[("acme api", "uses", "redis", None, Some(TS_2025_03))],
        },
        Fact {
            content: "after march 2025 the acme api caches sessions with the in-process moka cache",
            entities: &[("acme api", "Component"), ("moka", "Tool")],
            relations: &[("acme api", "uses", "moka", Some(TS_2025_03), None)],
        },
        Fact {
            content: "the search feature ranks merged results with reciprocal rank fusion",
            entities: &[("reciprocal rank fusion", "Concept")],
            relations: &[],
        },
    ]
}

/// `n` noise facts: give the baseline context a realistic weight and give the delegated search
/// distractors. Generated deterministically, chosen so their vocabulary does not overlap with the
/// gold/wrong keywords. At n=60 that is 15 services x 4 templates; beyond that it expands into
/// numbered service generations.
fn distractor_facts(n: usize) -> Vec<String> {
    const SERVICES: [&str; 15] = [
        "orion", "lyra", "vega", "altair", "deneb", "rigel", "castor", "pollux", "mira",
        "spica", "atlas", "electra", "maia", "merope", "alcyone",
    ];
    const TEAMS: [&str; 4] = ["falcon", "otter", "heron", "lynx"];
    const LANGS: [&str; 4] = ["go", "python", "typescript", "kotlin"];
    (0..n)
        .map(|i| {
            // Every 60 facts, bump the service name into a new generation (orion1 -> orion2) for unbounded expansion.
            let s = format!("{}{}", SERVICES[i % SERVICES.len()], i / 60 + 1);
            match (i / SERVICES.len()) % 4 {
                0 => format!("the {s} service exposes its api on port {}", 7000 + i),
                1 => format!(
                    "the {s} service is maintained by the {} team",
                    TEAMS[i % TEAMS.len()]
                ),
                2 => format!("the {s} service keeps logs for {} days", 7 + (i % 5) * 7),
                _ => format!("the {s} service is written in {}", LANGS[i % LANGS.len()]),
            }
        })
        .collect()
}

/// A single question. If `gold` is empty this is an unanswerable question (abstention expected).
struct Question {
    name: &'static str,
    text: &'static str,
    /// Keywords for judging a correct answer (any-of, lowercase). Correct takes precedence over stale.
    gold: &'static [&'static str],
    /// Old-version answer keywords (any-of) - answering with these is Stale (supersede failure).
    stale: &'static [&'static str],
}

fn questions() -> Vec<Question> {
    vec![
        Question {
            name: "direct: storage engine",
            text: "Which embedded storage engine does the acme project use?",
            gold: &["cozo"],
            stale: &[],
        },
        Question {
            name: "direct: message broker",
            text: "Which message broker does the ingest pipeline publish events through?",
            gold: &["nats"],
            stale: &[],
        },
        Question {
            name: "direct: dependency",
            text: "Which service does the billing service depend on for token validation?",
            gold: &["auth"],
            stale: &[],
        },
        Question {
            name: "supersede: deploy target",
            text: "Where is the acme api deployed today?",
            gold: &["fly"],
            stale: &["heroku"],
        },
        Question {
            name: "supersede: session cache",
            text: "What does the acme api currently use for session caching?",
            gold: &["moka", "in-process"],
            stale: &["redis"],
        },
        Question {
            name: "unanswerable: ci provider",
            text: "Which CI provider does the acme project use for continuous integration?",
            gold: &[],
            stale: &[],
        },
        Question {
            name: "unanswerable: frontend lead",
            text: "Who is the frontend lead of the acme project?",
            gold: &[],
            stale: &[],
        },
    ]
}

// --- Judging -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Verdict {
    /// Contains a gold keyword.
    Correct,
    /// Answered with the old-version value (supersede reconciliation failure).
    Stale,
    /// Answered, but neither gold nor stale.
    Wrong,
    /// Answered "I don't know" to an unanswerable (the expected behavior).
    Abstained,
    /// Made up a concrete answer to an unanswerable (hallucination).
    Hallucinated,
    /// No response / transport error.
    Error(String),
}

impl Verdict {
    fn label(&self) -> &str {
        match self {
            Verdict::Correct => "correct",
            Verdict::Stale => "stale",
            Verdict::Wrong => "wrong",
            Verdict::Abstained => "abstained",
            Verdict::Hallucinated => "hallucinated",
            Verdict::Error(_) => "error",
        }
    }
}

/// Abstention markers: if the answer contains any of these, judge it as "I don't know".
const ABSTAIN_MARKERS: [&str; 14] = [
    "i don't know",
    "i do not know",
    "don't know",
    "no information",
    "not found",
    "couldn't find",
    "could not find",
    "cannot find",
    "can't find",
    "no mention",
    "not specified",
    "not provided",
    "no record",
    "does not contain",
];

fn judge(q: &Question, answer: &str) -> Verdict {
    let a = answer.to_lowercase();
    if a.trim().is_empty() {
        return Verdict::Error("empty response".into());
    }
    if q.gold.is_empty() {
        if ABSTAIN_MARKERS.iter().any(|m| a.contains(m)) {
            Verdict::Abstained
        } else {
            Verdict::Hallucinated
        }
    } else if q.gold.iter().any(|g| a.contains(g)) {
        Verdict::Correct
    } else if q.stale.iter().any(|s| a.contains(s)) {
        Verdict::Stale
    } else {
        Verdict::Wrong
    }
}

// --- Result aggregation ------------------------------------------------------

struct QuestionResult {
    run: usize,
    question: &'static str,
    verdict: Verdict,
    answer: String,
    /// Tool-call log for the delegated condition (name(args) -> head of the result).
    tool_log: Vec<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
}

/// A bundle of results per (model, noise scale, condition, embedder). baseline uses no engine, hence "-".
struct ConditionResult {
    model: String,
    scale: usize,
    condition: &'static str,
    embedder: String,
    results: Vec<QuestionResult>,
}

impl ConditionResult {
    fn answerable(&self) -> Vec<&QuestionResult> {
        let answerable: Vec<&'static str> = questions()
            .iter()
            .filter(|q| !q.gold.is_empty())
            .map(|q| q.name)
            .collect();
        self.results
            .iter()
            .filter(|r| answerable.contains(&r.question))
            .collect()
    }

    fn unanswerable(&self) -> Vec<&QuestionResult> {
        let un: Vec<&'static str> = questions()
            .iter()
            .filter(|q| q.gold.is_empty())
            .map(|q| q.name)
            .collect();
        self.results.iter().filter(|r| un.contains(&r.question)).collect()
    }

    fn accuracy(&self) -> (usize, usize) {
        let a = self.answerable();
        let correct = a.iter().filter(|r| r.verdict == Verdict::Correct).count();
        (correct, a.len())
    }

    fn stale_count(&self) -> usize {
        self.answerable().iter().filter(|r| r.verdict == Verdict::Stale).count()
    }

    fn abstain(&self) -> (usize, usize) {
        let u = self.unanswerable();
        let ok = u.iter().filter(|r| r.verdict == Verdict::Abstained).count();
        (ok, u.len())
    }

    fn hallucinated(&self) -> usize {
        self.unanswerable()
            .iter()
            .filter(|r| r.verdict == Verdict::Hallucinated)
            .count()
    }

    fn total_tokens(&self) -> u64 {
        self.results
            .iter()
            .map(|r| r.prompt_tokens + r.completion_tokens)
            .sum()
    }

    fn mean_tokens_per_question(&self) -> f64 {
        if self.results.is_empty() {
            return 0.0;
        }
        self.total_tokens() as f64 / self.results.len() as f64
    }

    /// Tokens per correct: lower is more efficient. None if there are no correct answers.
    fn tokens_per_correct(&self) -> Option<f64> {
        let (correct, _) = self.accuracy();
        let abstained = self.abstain().0;
        let good = correct + abstained;
        if good == 0 {
            None
        } else {
            Some(self.total_tokens() as f64 / good as f64)
        }
    }
}

// --- Ollama bridge (same pattern as ollama_eval.rs, plus collecting token usage) ------

// --- Condition execution -----------------------------------------------------

/// baseline prompt template. {corpus} = list of core+noise facts, {question} = each question.
/// Execution and the report use the same source text (a reproducible problem definition).
const BASELINE_PROMPT: &str = "You are answering questions about a software project using the \
project notes below.\nAnswer concisely in one sentence. If the notes do not contain the \
answer, reply exactly: I don't know.\n\nPROJECT NOTES:\n{corpus}\n\nQUESTION: {question}";

/// delegated prompt template. Only {question} is substituted.
const DELEGATED_PROMPT: &str = "You are answering questions about a software project. A \
knowledge base is available through the provided tools. Use search_knowledge to look up \
relevant facts before answering; you may also use get_entity and traverse. Answer concisely \
in one sentence based only on what the tools return. If the knowledge base does not contain \
the answer, reply exactly: I don't know.\n\nQUESTION: {question}";

/// baseline: inject the whole corpus into the prompt and ask in a single turn.
async fn run_baseline(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    corpus_text: &str,
    q: &Question,
    run: usize,
) -> QuestionResult {
    let prompt = BASELINE_PROMPT
        .replace("{corpus}", corpus_text)
        .replace("{question}", q.text);
    let messages = json!([{ "role": "user", "content": prompt }]);
    match chat(http, base, model, &messages, None).await {
        Ok((msg, u)) => {
            let answer = msg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            QuestionResult {
                run,
                question: q.name,
                verdict: judge(q, &answer),
                answer,
                tool_log: Vec::new(),
                prompt_tokens: u.prompt,
                completion_tokens: u.completion,
            }
        }
        Err(e) => QuestionResult {
            run,
            question: q.name,
            verdict: Verdict::Error(e.clone()),
            answer: format!("[error] {e}"),
            tool_log: Vec::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
        },
    }
}

/// delegated: give only the tools and ask. Repeat execute-and-feed-back while tool_calls keep
/// coming, and when a text answer appears, score that as the final answer.
async fn run_delegated(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    client: &RunningService<RoleClient, ()>,
    tools: &Value,
    q: &Question,
    run: usize,
) -> QuestionResult {
    let prompt = DELEGATED_PROMPT.replace("{question}", q.text);
    let mut messages = json!([{ "role": "user", "content": prompt }]);
    let mut tool_log = Vec::new();
    let (mut pt_sum, mut ct_sum) = (0u64, 0u64);

    for _round in 0..MAX_ROUNDS {
        let (msg, u) = match chat(http, base, model, &messages, Some(tools)).await {
            Ok(v) => v,
            Err(e) => {
                return QuestionResult {
                    run,
                    question: q.name,
                    verdict: Verdict::Error(e.clone()),
                    answer: format!("[error] {e}"),
                    tool_log,
                    prompt_tokens: pt_sum,
                    completion_tokens: ct_sum,
                }
            }
        };
        pt_sum += u.prompt;
        ct_sum += u.completion;

        let calls = tool_calls(&msg);
        if calls.is_empty() {
            let answer = msg
                .get("content")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            return QuestionResult {
                run,
                question: q.name,
                verdict: judge(q, &answer),
                answer,
                tool_log,
                prompt_tokens: pt_sum,
                completion_tokens: ct_sum,
            };
        }
        push_message(&mut messages, msg.clone());
        for (id, name, args) in &calls {
            let result = exec_tool(client, name, args).await;
            let head: String = result.chars().take(160).collect();
            tool_log.push(format!("{name}({args}) -> {head}"));
            push_message(
                &mut messages,
                json!({ "role": "tool", "tool_call_id": id, "content": result }),
            );
        }
    }

    QuestionResult {
        run,
        question: q.name,
        verdict: Verdict::Error(format!("no text answer within {MAX_ROUNDS} rounds")),
        answer: "[error] only repeated tool calls".into(),
        tool_log,
        prompt_tokens: pt_sum,
        completion_tokens: ct_sum,
    }
}

/// Deterministically loads the corpus into the engine (decoupled from model ingestion quality - isolates the recall side only).
fn load_corpus(engine: &Engine, scale: usize) {
    for f in core_facts() {
        engine
            .observe(ObserveInput {
                content: f.content.into(),
                workspace: Some(WS.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: f
                    .entities
                    .iter()
                    .map(|(n, t)| EntityInput { description: None,
                        name: (*n).into(),
                        kind: Some((*t).into()),
                    })
                    .collect(),
                relations: f
                    .relations
                    .iter()
                    .map(|(from, kind, to, vf, vt)| RelationInput { description: None,
                        from: (*from).into(),
                        kind: (*kind).into(),
                        to: (*to).into(),
                        valid_from: *vf,
                        valid_to: *vt,
                    })
                    .collect(),
            })
            .expect("load core fact");
    }
    for content in distractor_facts(scale) {
        engine
            .observe(ObserveInput {
                content,
                workspace: Some(WS.into()),
                source_ref: None,
                confidence: None,
                on_behalf_of: None,
                derived_from: vec![],
                entities: vec![],
                relations: vec![],
            })
            .expect("load distractor");
    }
}

/// Corpus text for the baseline prompt (core + noise, same order as ingestion).
fn corpus_text(scale: usize) -> String {
    let mut lines: Vec<String> = core_facts().iter().map(|f| format!("- {}", f.content)).collect();
    lines.extend(distractor_facts(scale).iter().map(|d| format!("- {d}")));
    lines.join("\n")
}

// --- Report ------------------------------------------------------------------

/// Builds the quantitative summary + qualitative transcript as markdown.
fn render_report(conditions: &[ConditionResult], runs: usize) -> String {
    let mut md = String::new();
    md.push_str("# delegation eval report\n\n");
    md.push_str(&format!(
        "- {} questions (answerable {}, unanswerable {}), {} runs\n",
        questions().len(),
        questions().iter().filter(|q| !q.gold.is_empty()).count(),
        questions().iter().filter(|q| q.gold.is_empty()).count(),
        runs
    ));
    md.push_str(&format!("- {} core facts; the noise scale is the scale column of the table\n\n", core_facts().len()));

    // Quantitative summary table.
    md.push_str("## Quantitative summary\n\n");
    md.push_str(
        "| model | scale | condition | embedder | accuracy(answerable) | stale | abstention(unanswerable) | hallucination | \
         mean tokens/question | tokens/correct |\n",
    );
    md.push_str("|---|---|---|---|---|---|---|---|---|---|\n");
    for c in conditions {
        let (correct, total) = c.accuracy();
        let (abstained, un_total) = c.abstain();
        let tpc = c
            .tokens_per_correct()
            .map(|t| format!("{t:.0}"))
            .unwrap_or_else(|| "-".into());
        md.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {} | {}/{} | {} | {:.0} | {} |\n",
            c.model,
            c.scale,
            c.condition,
            c.embedder,
            correct,
            total,
            c.stale_count(),
            abstained,
            un_total,
            c.hallucinated(),
            c.mean_tokens_per_question(),
            tpc
        ));
    }

    // Problem definition: the prompts and question source text used identically for all models.
    md.push_str("\n## Prompts used (identical for all models)\n\n");
    md.push_str("baseline ({corpus} = injected list of core+noise facts):\n\n```text\n");
    md.push_str(BASELINE_PROMPT);
    md.push_str("\n```\n\ndelegated (the tool schemas are auto-converted from the MCP surface and passed separately):\n\n```text\n");
    md.push_str(DELEGATED_PROMPT);
    md.push_str("\n```\n\nQuestion source text:\n\n");
    for q in questions() {
        md.push_str(&format!("- {}: {}\n", q.name, q.text));
    }

    // Qualitative transcript.
    md.push_str("\n## Transcript (for qualitative review)\n");
    for c in conditions {
        md.push_str(&format!(
            "\n### {} / scale {} / {} / {}\n",
            c.model, c.scale, c.condition, c.embedder
        ));
        for r in &c.results {
            md.push_str(&format!(
                "\n**[run {}] {}** - verdict: `{}`\n\n",
                r.run,
                r.question,
                r.verdict.label()
            ));
            if let Verdict::Error(e) = &r.verdict {
                md.push_str(&format!("- error: {e}\n"));
            }
            for t in &r.tool_log {
                md.push_str(&format!("- tool: `{t}`\n"));
            }
            md.push_str(&format!(
                "- answer: {}\n- tokens: prompt {} / completion {}\n",
                r.answer.replace('\n', " "),
                r.prompt_tokens,
                r.completion_tokens
            ));
        }
    }
    md
}

/// Writes the report to target/eval-reports/ and refreshes the index. Returns the path.
fn write_report(md: &str) -> std::path::PathBuf {
    report::write_report("delegation_eval.md", md)
}

// --- Main --------------------------------------------------------------------

#[tokio::test]
#[ignore = "requires local Ollama - manual A/B eval of knowledge-delegation gain"]
async fn delegation_beats_context_stuffing() {
    let base = std::env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE.to_string());
    let models_env = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| DEFAULT_MODELS.to_string());
    let models: Vec<&str> = models_env
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let runs: usize = std::env::var("EVAL_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let scales: Vec<usize> = std::env::var("EVAL_SCALES")
        .unwrap_or_else(|_| "60".to_string())
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    // Embedder A/B axis. Built here and shared via clone so the real-model init cost is paid only once.
    let embedder_names: Vec<String> = std::env::var("EVAL_EMBEDDERS")
        .unwrap_or_else(|_| "hashing".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let embedder_list: Vec<(String, std::sync::Arc<dyn supragnosis_core::EmbeddingProvider>)> =
        embedder_names
            .iter()
            .map(|n| (n.clone(), supragnosis_e2e::embedders::make_embedder(n)))
            .collect();

    let http = reqwest::Client::builder()
        // baseline at large scale takes long to evaluate the prompt - allow generous time.
        .timeout(Duration::from_secs(600))
        .build()
        .expect("http client");

    if !ollama_reachable(&http, &base).await {
        eprintln!("[skip] cannot reach Ollama ({base}) - rerun after `ollama serve`");
        return;
    }

    let mut conditions: Vec<ConditionResult> = Vec::new();

    for model in &models {
        for &scale in &scales {
            eprintln!("\n=== model: {model} / noise {scale} facts ===");
            let corpus = corpus_text(scale);

            // baseline is independent of the engine (embedder) - measured once per (model, scale).
            let mut baseline = ConditionResult {
                model: model.to_string(),
                scale,
                condition: "baseline",
                embedder: "-".into(),
                results: Vec::new(),
            };
            for run in 1..=runs {
                for q in questions() {
                    let rb = run_baseline(&http, &base, model, &corpus, &q, run).await;
                    eprintln!(
                        "  [baseline           ] run{run} {:<28} {:<12} ({}tk)",
                        q.name,
                        rb.verdict.label(),
                        rb.prompt_tokens + rb.completion_tokens
                    );
                    baseline.results.push(rb);
                }
            }
            conditions.push(baseline);

            // delegated is measured on a fresh engine per embedder (isolating (model, scale, embedder)).
            for (emb_name, emb) in &embedder_list {
                let engine = Arc::new(
                    Engine::new(Arc::new(InMemoryStore::new()), "delegation-eval", WS)
                        .with_embedder(emb.clone()),
                );
                load_corpus(&engine, scale);
                let (client, server) = serve_engine(engine).await;
                let tools = openai_tools(&client).await;

                let mut delegated = ConditionResult {
                    model: model.to_string(),
                    scale,
                    condition: "delegated",
                    embedder: emb_name.clone(),
                    results: Vec::new(),
                };
                for run in 1..=runs {
                    for q in questions() {
                        let rd =
                            run_delegated(&http, &base, model, &client, &tools, &q, run).await;
                        eprintln!(
                            "  [delegated/{emb_name:<9}] run{run} {:<28} {:<12} ({}tk, {} tool calls)",
                            q.name,
                            rd.verdict.label(),
                            rd.prompt_tokens + rd.completion_tokens,
                            rd.tool_log.len()
                        );
                        delegated.results.push(rd);
                    }
                }
                conditions.push(delegated);

                let _ = client.cancel().await;
                let _ = server.await;
            }
        }
    }

    // Quantitative comparison table (stdout).
    eprintln!("\n=== comparison (accuracy / stale / abstention / hallucination / mean tokens / tokens-per-correct) ===");
    for c in &conditions {
        let (correct, total) = c.accuracy();
        let (abst, un) = c.abstain();
        let tpc = c
            .tokens_per_correct()
            .map(|t| format!("{t:.0}"))
            .unwrap_or_else(|| "-".into());
        eprintln!(
            "  {:<14} n={:<5} {:<10} emb={:<9} acc {}/{}  stale {}  abstain {}/{}  halluc {}  tk/q {:.0}  tk/correct {}",
            c.model,
            c.scale,
            c.condition,
            c.embedder,
            correct,
            total,
            c.stale_count(),
            abst,
            un,
            c.hallucinated(),
            c.mean_tokens_per_question(),
            tpc
        );
    }

    let report = render_report(&conditions, runs);
    let path = write_report(&report);
    eprintln!("\n[report] {}", path.display());

    // The point of the assertion is to measure "is delegation a gain" - it does not enforce model
    // quality (the numbers surface in the comparison table/report). But if no model produces even
    // one valid answer under any condition, the harness/bridge is broken, so fail.
    let any_answer = conditions
        .iter()
        .any(|c| c.results.iter().any(|r| !matches!(r.verdict, Verdict::Error(_))));
    assert!(
        any_answer,
        "no model produced a valid answer - check the Ollama bridge/harness"
    );
}
