//! Delegation eval: "지식 위임이 실제로 이득인가"를 정량/정성으로 채점한다.
//!
//! 논지: 소형 모델이 지식을 supragnosis MCP 에 위임하면, 전체 이력을 컨텍스트에
//! 넣지 않고도 같은(또는 더 나은) 정확도를 더 적은 토큰으로 낼 수 있다.
//! 이 하네스는 그 논지를 A/B 로 직접 잰다:
//!
//!   - baseline  조건: 코퍼스 전체(핵심 사실 + 잡음 사실)를 프롬프트에 주입하고 질문.
//!   - delegated 조건: 같은 코퍼스를 엔진에 미리 적재(결정적 - 모델의 적재 품질과 분리)
//!     하고, 모델은 MCP 도구(search_knowledge 등)로 필요한 만큼만 꺼내 답한다.
//!
//! 질문 카테고리와 정량 지표:
//!   - direct      : 단순 회상 정확도 (정답 키워드 포함 여부).
//!   - supersede   : 사실이 갱신된 경우 최신 값을 답하는가 (stale 답변율 - 원칙 3/6:
//!     충돌은 정보다, 모델이 두 관측을 화해시켜야 한다).
//!   - unanswerable: 코퍼스에 없는 질문에 "모른다"고 답하는가 (부작위/환각율 - 위임
//!     구조 최대 리스크는 검색 없이 지어내는 것).
//!   - 토큰 비용   : 조건별 프롬프트+생성 토큰 합계, 정답당 토큰(tokens-per-correct).
//!
//! 정성 평가: 전체 트랜스크립트(질문/도구 호출/인자/결과/최종 답변/판정)를 마크다운
//! 리포트로 남긴다 -> target/eval-reports/delegation_eval.md (사람이 리뷰하는 산출물).
//!
//! 판정은 결정적 키워드 매칭이다(정량 재현성). 자유 서술의 미묘한 오답은 리포트의
//! 정성 리뷰로 잡는다 - LLM-judge 는 비결정성을 더하므로 의도적으로 넣지 않았다.
//!
//! 비결정적(모델)이고 로컬 Ollama 가 필요하므로 기본 실행에서 제외한다.
//! 실행:
//!   OLLAMA_MODELS=gemma4,qwen2.5:3b,llama3.2:3b \
//!     cargo test -p supragnosis-e2e --test delegation_eval -- --ignored --nocapture
//! 선택 env:
//!   OLLAMA_BASE_URL (기본 http://localhost:11434)
//!   OLLAMA_MODELS   (콤마 구분, 기본 gemma4)
//!   EVAL_RUNS       (반복 횟수, 기본 1 - 소형 모델 흔들림을 pass-rate 로 재려면 3+)
//!   EVAL_SCALES     (콤마 구분 잡음 사실 수, 기본 60 - 예: 60,300,1000. baseline 은
//!                    코퍼스에 선형으로 비싸지고 컨텍스트 윈도우를 넘으면 무너지는 반면
//!                    delegated 는 상수임을 손익분기 곡선으로 보이기 위한 축)
//!
//! Ollama 가 안 떠 있으면 조용히 통과(skip)한다 - CI 를 깨지 않기 위해서다.

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
use supragnosis_embed::HashingEmbedder;
use supragnosis_engine::{Engine, EntityInput, ObserveInput, RelationInput};
use supragnosis_store::InMemoryStore;

const DEFAULT_MODELS: &str = "gemma4";
const WS: &str = "eval";
/// delegated 조건에서 도구 왕복 최대 횟수(무한 루프 가드).
const MAX_ROUNDS: usize = 5;

// 유효구간 픽스처용 epoch millis 상수.
const TS_2024_01: u64 = 1_704_067_200_000; // 2024-01-01
const TS_2025_03: u64 = 1_740_787_200_000; // 2025-03-01
const TS_2025_06: u64 = 1_748_736_000_000; // 2025-06-01

// --- 픽스처: 코퍼스와 질문 ---------------------------------------------------

/// 관계 픽스처: (from, kind, to, valid_from, valid_to).
type FactRelation = (&'static str, &'static str, &'static str, Option<u64>, Option<u64>);

/// 핵심 사실 한 건: 관측 본문 + 동봉 엔티티/관계(유효구간 포함).
struct Fact {
    content: &'static str,
    entities: &'static [(&'static str, &'static str)],
    relations: &'static [FactRelation],
}

/// 핵심 사실. supersede 쌍(4-5, 6-7)은 날짜를 본문에 명시해 두 관측이 모두 회수됐을 때
/// 모델이 최신을 고를 수 있게 한다(엔진의 자동 반증 종료는 M3 - 지금은 모델의 몫).
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
        // supersede 쌍 1: 배포 대상 이전.
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
        // supersede 쌍 2: 세션 캐시 교체.
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

/// 잡음 사실 `n`건: baseline 컨텍스트를 현실적인 무게로 만들고 delegated 검색에
/// distractor 를 준다. 결정적으로 생성하며, 정답/오답 키워드와 어휘가 겹치지 않게 고른다.
/// n=60 이면 서비스 15개 x 템플릿 4개, 그 이상은 번호 붙은 서비스 세대로 확장된다.
fn distractor_facts(n: usize) -> Vec<String> {
    const SERVICES: [&str; 15] = [
        "orion", "lyra", "vega", "altair", "deneb", "rigel", "castor", "pollux", "mira",
        "spica", "atlas", "electra", "maia", "merope", "alcyone",
    ];
    const TEAMS: [&str; 4] = ["falcon", "otter", "heron", "lynx"];
    const LANGS: [&str; 4] = ["go", "python", "typescript", "kotlin"];
    (0..n)
        .map(|i| {
            // 60건마다 새 세대(orion1 -> orion2)로 서비스명을 늘려 무한 확장한다.
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

/// 질문 한 건. `gold` 가 비면 unanswerable(부작위 기대) 질문이다.
struct Question {
    name: &'static str,
    text: &'static str,
    /// 정답 판정 키워드(any-of, 소문자). 정답이 stale 보다 우선한다.
    gold: &'static [&'static str],
    /// 구버전 정답 키워드(any-of) - 이걸 답하면 Stale (supersede 실패).
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

// --- 판정 --------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Verdict {
    /// 정답 키워드 포함.
    Correct,
    /// 구버전 정답을 답함 (supersede 화해 실패).
    Stale,
    /// 답했지만 정답도 stale 도 아님.
    Wrong,
    /// unanswerable 에 "모른다"로 답함 (기대 행동).
    Abstained,
    /// unanswerable 에 구체적 답을 지어냄 (환각).
    Hallucinated,
    /// 응답 없음/전송 오류.
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

/// 부작위 표지: 답변에 이 중 하나가 보이면 "모른다"로 판정한다.
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
        return Verdict::Error("빈 응답".into());
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

// --- 결과 집계 ---------------------------------------------------------------

struct QuestionResult {
    run: usize,
    question: &'static str,
    verdict: Verdict,
    answer: String,
    /// delegated 조건의 도구 호출 로그 (이름(인자) -> 결과 앞부분).
    tool_log: Vec<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
}

/// (모델, 잡음 규모, 조건)별 결과 묶음.
struct ConditionResult {
    model: String,
    scale: usize,
    condition: &'static str,
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

    /// 정답당 토큰: 낮을수록 효율적. 정답이 없으면 None.
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

// --- Ollama 브리지 (ollama_eval.rs 와 같은 패턴, 토큰 usage 회수를 더함) ------

// --- 조건 실행 ---------------------------------------------------------------

/// baseline: 코퍼스 전체를 프롬프트에 주입하고 단일턴으로 질문한다.
async fn run_baseline(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    corpus_text: &str,
    q: &Question,
    run: usize,
) -> QuestionResult {
    let prompt = format!(
        "You are answering questions about a software project using the project notes below.\n\
         Answer concisely in one sentence. If the notes do not contain the answer, reply \
         exactly: I don't know.\n\nPROJECT NOTES:\n{corpus_text}\n\nQUESTION: {}",
        q.text
    );
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

/// delegated: 도구만 주고 질문한다. tool_calls 가 나오는 동안 실행-되먹임을 반복하고,
/// 텍스트 답이 나오면 그걸 최종 답으로 채점한다.
async fn run_delegated(
    http: &reqwest::Client,
    base: &str,
    model: &str,
    client: &RunningService<RoleClient, ()>,
    tools: &Value,
    q: &Question,
    run: usize,
) -> QuestionResult {
    let prompt = format!(
        "You are answering questions about a software project. A knowledge base is available \
         through the provided tools. Use search_knowledge to look up relevant facts before \
         answering; you may also use get_entity and traverse. Answer concisely in one sentence \
         based only on what the tools return. If the knowledge base does not contain the \
         answer, reply exactly: I don't know.\n\nQUESTION: {}",
        q.text
    );
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
        verdict: Verdict::Error(format!("{MAX_ROUNDS} 라운드 내에 텍스트 답 없음")),
        answer: "[error] 도구 호출만 반복".into(),
        tool_log,
        prompt_tokens: pt_sum,
        completion_tokens: ct_sum,
    }
}

/// 코퍼스를 엔진에 결정적으로 적재한다(모델 적재 품질과 분리 - 회수 측만 격리 측정).
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
                    .map(|(n, t)| EntityInput {
                        name: (*n).into(),
                        kind: Some((*t).into()),
                    })
                    .collect(),
                relations: f
                    .relations
                    .iter()
                    .map(|(from, kind, to, vf, vt)| RelationInput {
                        from: (*from).into(),
                        kind: (*kind).into(),
                        to: (*to).into(),
                        valid_from: *vf,
                        valid_to: *vt,
                    })
                    .collect(),
            })
            .expect("core fact 적재");
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
            .expect("distractor 적재");
    }
}

/// baseline 프롬프트용 코퍼스 텍스트(핵심 + 잡음, 적재 순서와 동일).
fn corpus_text(scale: usize) -> String {
    let mut lines: Vec<String> = core_facts().iter().map(|f| format!("- {}", f.content)).collect();
    lines.extend(distractor_facts(scale).iter().map(|d| format!("- {d}")));
    lines.join("\n")
}

// --- 리포트 ------------------------------------------------------------------

/// 정량 요약 + 정성 트랜스크립트를 마크다운으로 만든다.
fn render_report(conditions: &[ConditionResult], runs: usize) -> String {
    let mut md = String::new();
    md.push_str("# delegation eval 리포트\n\n");
    md.push_str(&format!(
        "- 질문 {}개 (answerable {}, unanswerable {}), 반복 {}회\n",
        questions().len(),
        questions().iter().filter(|q| !q.gold.is_empty()).count(),
        questions().iter().filter(|q| q.gold.is_empty()).count(),
        runs
    ));
    md.push_str(&format!("- 핵심 사실 {}건, 잡음 규모는 표의 scale 열\n\n", core_facts().len()));

    // 정량 요약표.
    md.push_str("## 정량 요약\n\n");
    md.push_str(
        "| 모델 | scale | 조건 | 정확도(answerable) | stale | 부작위(unanswerable) | 환각 | \
         평균 토큰/질문 | 토큰/정답 |\n",
    );
    md.push_str("|---|---|---|---|---|---|---|---|---|\n");
    for c in conditions {
        let (correct, total) = c.accuracy();
        let (abstained, un_total) = c.abstain();
        let tpc = c
            .tokens_per_correct()
            .map(|t| format!("{t:.0}"))
            .unwrap_or_else(|| "-".into());
        md.push_str(&format!(
            "| {} | {} | {} | {}/{} | {} | {}/{} | {} | {:.0} | {} |\n",
            c.model,
            c.scale,
            c.condition,
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

    // 정성 트랜스크립트.
    md.push_str("\n## 트랜스크립트 (정성 리뷰용)\n");
    for c in conditions {
        md.push_str(&format!(
            "\n### {} / scale {} / {}\n",
            c.model, c.scale, c.condition
        ));
        for r in &c.results {
            md.push_str(&format!(
                "\n**[run {}] {}** - 판정: `{}`\n\n",
                r.run,
                r.question,
                r.verdict.label()
            ));
            if let Verdict::Error(e) = &r.verdict {
                md.push_str(&format!("- 오류: {e}\n"));
            }
            for t in &r.tool_log {
                md.push_str(&format!("- 도구: `{t}`\n"));
            }
            md.push_str(&format!(
                "- 답변: {}\n- 토큰: prompt {} / completion {}\n",
                r.answer.replace('\n', " "),
                r.prompt_tokens,
                r.completion_tokens
            ));
        }
    }
    md
}

/// 리포트를 target/eval-reports/ 에 쓰고 index 를 갱신한다. 경로를 돌려준다.
fn write_report(md: &str) -> std::path::PathBuf {
    report::write_report("delegation_eval.md", md)
}

// --- 메인 --------------------------------------------------------------------

#[tokio::test]
#[ignore = "로컬 Ollama 필요 - 지식 위임 이득 A/B 수동 eval"]
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

    let http = reqwest::Client::builder()
        // 큰 scale 의 baseline 은 프롬프트 평가가 길다 - 넉넉히 잡는다.
        .timeout(Duration::from_secs(600))
        .build()
        .expect("http client");

    if !ollama_reachable(&http, &base).await {
        eprintln!("[skip] Ollama 에 연결 불가({base}) - `ollama serve` 후 재실행");
        return;
    }

    let mut conditions: Vec<ConditionResult> = Vec::new();

    for model in &models {
        for &scale in &scales {
            eprintln!("\n=== 모델: {model} / 잡음 {scale}건 ===");
            let corpus = corpus_text(scale);

            // delegated 조건용 MCP 서버((모델, 규모)별 새 엔진 - 교차 오염 방지).
            let engine = Arc::new(
                Engine::new(Arc::new(InMemoryStore::new()), "delegation-eval", WS)
                    .with_embedder(Arc::new(HashingEmbedder::default())),
            );
            load_corpus(&engine, scale);
            let (client, server) = serve_engine(engine).await;
            let tools = openai_tools(&client).await;

            let mut baseline = ConditionResult {
                model: model.to_string(),
                scale,
                condition: "baseline",
                results: Vec::new(),
            };
            let mut delegated = ConditionResult {
                model: model.to_string(),
                scale,
                condition: "delegated",
                results: Vec::new(),
            };

            for run in 1..=runs {
                for q in questions() {
                    let rb = run_baseline(&http, &base, model, &corpus, &q, run).await;
                    eprintln!(
                        "  [baseline ] run{run} {:<28} {:<12} ({}tk)",
                        q.name,
                        rb.verdict.label(),
                        rb.prompt_tokens + rb.completion_tokens
                    );
                    baseline.results.push(rb);

                    let rd = run_delegated(&http, &base, model, &client, &tools, &q, run).await;
                    eprintln!(
                        "  [delegated] run{run} {:<28} {:<12} ({}tk, 도구 {}회)",
                        q.name,
                        rd.verdict.label(),
                        rd.prompt_tokens + rd.completion_tokens,
                        rd.tool_log.len()
                    );
                    delegated.results.push(rd);
                }
            }

            conditions.push(baseline);
            conditions.push(delegated);

            let _ = client.cancel().await;
            let _ = server.await;
        }
    }

    // 정량 비교표 (stdout).
    eprintln!("\n=== 비교 (정확도 / stale / 부작위 / 환각 / 평균토큰 / 토큰-정답) ===");
    for c in &conditions {
        let (correct, total) = c.accuracy();
        let (abst, un) = c.abstain();
        let tpc = c
            .tokens_per_correct()
            .map(|t| format!("{t:.0}"))
            .unwrap_or_else(|| "-".into());
        eprintln!(
            "  {:<14} n={:<5} {:<10} acc {}/{}  stale {}  abstain {}/{}  halluc {}  tk/q {:.0}  tk/correct {}",
            c.model,
            c.scale,
            c.condition,
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

    // 검증 목적은 "위임이 이득인가"의 측정이다 - 모델 품질을 강제하지 않는다(수치는
    // 비교표/리포트로 드러난다). 다만 어떤 모델도 어떤 조건에서도 유효한 답을 하나도
    // 못 내면 하네스/브리지가 깨진 것이라 실패.
    let any_answer = conditions
        .iter()
        .any(|c| c.results.iter().any(|r| !matches!(r.verdict, Verdict::Error(_))));
    assert!(
        any_answer,
        "어떤 모델도 유효한 답을 내지 못함 - Ollama 브리지/하네스 점검 필요"
    );
}
