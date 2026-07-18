//! 평가 산출물(마크다운 리포트, HTML 갤러리/뷰어, 데모 파일)의 출력과 인덱스.
//! target/ 아래에 두어 저장소에는 실리지 않는다(저장소엔 완성물만).
//!
//! 각 eval 은 [`write_report`] 로 산출물을 쓴다 - 쓸 때마다 디렉터리를 스캔해
//! `index.html` 대시보드를 재생성하므로, 스위트를 어떤 조합으로 돌리든
//! `target/eval-reports/index.html` 하나가 항상 최신 목차다.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// target/eval-reports/ 를 만들어 돌려준다.
pub fn report_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/eval-reports");
    std::fs::create_dir_all(&dir).expect("리포트 디렉터리 생성");
    dir
}

/// 산출물을 쓰고 index.html 을 갱신한다. 쓴 경로를 돌려준다.
pub fn write_report(filename: &str, content: &str) -> PathBuf {
    let dir = report_dir();
    let path = dir.join(filename);
    std::fs::write(&path, content).expect("리포트 쓰기");
    refresh_index();
    path
}

/// 알려진 산출물의 (파일명, 제목, 설명). 스캔 시 이 순서로 앞에 배치한다.
const KNOWN: [(&str, &str, &str); 6] = [
    (
        "physics_gallery.html",
        "physics 데모 갤러리",
        "모델 x 조건별 물리 데모가 실제로 돌아가는 정성 비교 (행동 배지 포함)",
    ),
    (
        "physics_coding_eval.md",
        "physics coding eval",
        "공통 온톨로지 위임이 코딩 산출물에 반영되는가 - 설계 지문 + 행동 채점",
    ),
    (
        "ontology_viewer.html",
        "온톨로지 뷰어",
        "모델이 작업 부산물로 지은 온톨로지 그래프 (포스 레이아웃, 모델 탭)",
    ),
    (
        "ontology_build_eval.md",
        "ontology build eval",
        "적재(쓰기) 품질 - observe 호출률, 커버리지, 고립/중복",
    ),
    (
        "delegation_eval.md",
        "delegation eval",
        "지식 위임 이득 A/B - 회수 정확도, stale, 부작위/환각, 토큰 손익",
    ),
    (
        "ollama_eval.md",
        "ollama tool-use eval",
        "소형 모델의 MCP 도구 사용 정확도 채점표",
    ),
];

/// epoch 초를 "YYYY-MM-DD HH:MM KST" 로 (UTC+9 고정).
fn format_kst(epoch: u64) -> String {
    let s = epoch + 9 * 3600;
    let (days, rem) = (s / 86400, s % 86400);
    let (hh, mm) = (rem / 3600, (rem % 3600) / 60);
    // civil-from-days (Howard Hinnant 알고리즘).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02} KST")
}

fn mtime_epoch(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// eval-reports/ 를 스캔해 index.html 을 재생성한다.
///
/// - 알려진 산출물은 제목/설명과 함께 카드로, 마크다운 리포트는 본문을 인라인으로 펼친다.
/// - 목록에 없는 여분의 .md/.html 도 뒤에 일반 카드로 붙는다(누락 방지).
pub fn refresh_index() {
    let dir = report_dir();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 목차 항목: 알려진 산출물(존재하는 것만) + 알려지지 않은 여분의 .md/.html.
    let mut entries: Vec<(String, String, String)> = KNOWN
        .iter()
        .filter(|(f, _, _)| dir.join(f).exists())
        .map(|(f, t, d)| (f.to_string(), t.to_string(), d.to_string()))
        .collect();
    let mut extras: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| {
                    (n.ends_with(".md") || n.ends_with(".html"))
                        && n != "index.html"
                        && !KNOWN.iter().any(|(f, _, _)| f == n)
                })
                .collect()
        })
        .unwrap_or_default();
    extras.sort();
    entries.extend(extras.into_iter().map(|f| (f.clone(), f, "(자동 수집)".to_string())));

    let mut cards = String::new();
    for (file, title, desc) in &entries {
        let path = dir.join(file);
        let stamp = mtime_epoch(&path).map(format_kst).unwrap_or_default();
        let body = if file.ends_with(".md") {
            let md = std::fs::read_to_string(&path).unwrap_or_default();
            format!(
                "<details open><summary>리포트 본문</summary><pre>{}</pre></details>",
                html_escape(&md)
            )
        } else {
            String::new()
        };
        cards.push_str(&format!(
            "<section><h2><a href=\"{file}\">{title}</a></h2>\
             <p class=\"meta\">{file} - {stamp}</p><p>{desc}</p>{body}</section>\n"
        ));
    }

    let html = format!(
        r##"<!doctype html>
<meta charset="utf-8">
<title>supragnosis e2e 리포트</title>
<style>
  :root {{ color-scheme: dark; }}
  body {{ margin: 0 auto; max-width: 920px; padding: 24px 16px; background: #101216;
         color: #d8dee9; font: 14px/1.6 system-ui, sans-serif; }}
  h1 {{ font-size: 18px; margin: 0 0 2px; }}
  p.sub {{ color: #9aa5b1; font-size: 12.5px; margin: 0 0 20px; }}
  section {{ background: #171a20; border: 1px solid #2a2f38; border-radius: 8px;
            padding: 14px 16px; margin-bottom: 14px; }}
  h2 {{ font-size: 15px; margin: 0 0 2px; }}
  a {{ color: #7aa2f7; text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
  .meta {{ color: #6b7480; font-size: 12px; margin: 0 0 6px; }}
  section p {{ margin: 0 0 6px; }}
  details {{ margin-top: 8px; }}
  summary {{ cursor: pointer; color: #9aa5b1; font-size: 12.5px; }}
  pre {{ overflow-x: auto; background: #101216; border: 1px solid #262b33;
        border-radius: 6px; padding: 10px 12px; font-size: 12px; line-height: 1.5; }}
</style>
<h1>supragnosis e2e 리포트</h1>
<p class="sub">실모델 종단 측정 스위트의 산출물 목차 - 생성 {stamp}.
`cargo test -p supragnosis-e2e -- --ignored` 계열 실행이 이 페이지를 갱신한다.</p>
{cards}"##,
        stamp = format_kst(now),
    );
    std::fs::write(dir.join("index.html"), html).expect("index 쓰기");
}
