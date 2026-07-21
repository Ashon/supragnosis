//! Output and indexing of eval artifacts (markdown reports, HTML gallery/viewer, demo files).
//! Kept under target/ so they are not committed to the repository (only finished work is).
//!
//! Each eval writes its artifacts via [`write_report`] - every write scans the directory and
//! regenerates the `index.html` dashboard, so no matter which combination of the suite you run,
//! the single `target/eval-reports/index.html` is always the up-to-date table of contents.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Creates target/eval-reports/ and returns it.
pub fn report_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/eval-reports");
    std::fs::create_dir_all(&dir).expect("create report directory");
    dir
}

/// Writes an artifact and refreshes index.html. Returns the written path.
pub fn write_report(filename: &str, content: &str) -> PathBuf {
    let dir = report_dir();
    let path = dir.join(filename);
    std::fs::write(&path, content).expect("write report");
    refresh_index();
    path
}

/// (filename, title, description) of the known artifacts. Placed up front in this order when scanning.
const KNOWN: [(&str, &str, &str); 6] = [
    (
        "physics_gallery.html",
        "physics demo gallery",
        "qualitative comparison of the physics demos actually running, per model x condition (with behavior badges)",
    ),
    (
        "physics_coding_eval.md",
        "physics coding eval",
        "whether a shared-ontology delegation shows up in coding output - design fingerprints + behavior scoring",
    ),
    (
        "ontology_viewer.html",
        "ontology viewer",
        "the ontology graph a model built as a by-product of its work (force layout, model tabs)",
    ),
    (
        "ontology_build_eval.md",
        "ontology build eval",
        "ingestion (write) quality - observe call rate, coverage, isolated/duplicate nodes",
    ),
    (
        "delegation_eval.md",
        "delegation eval",
        "knowledge-delegation gain A/B - recall accuracy, stale, abstention/hallucination, token cost/benefit",
    ),
    (
        "ollama_eval.md",
        "ollama tool-use eval",
        "scorecard of small-model MCP tool-use accuracy",
    ),
];

/// Formats epoch seconds as "YYYY-MM-DD HH:MM KST" (fixed UTC+9).
fn format_kst(epoch: u64) -> String {
    let s = epoch + 9 * 3600;
    let (days, rem) = (s / 86400, s % 86400);
    let (hh, mm) = (rem / 3600, (rem % 3600) / 60);
    // civil-from-days (Howard Hinnant algorithm).
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

/// Renders markdown to HTML (with the GFM tables extension).
fn render_markdown_html(md: &str) -> String {
    use pulldown_cmark::{html, Options, Parser};
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    let mut out = String::new();
    html::push_html(&mut out, Parser::new_ext(md, opts));
    out
}

/// Scans eval-reports/ and regenerates index.html.
///
/// - Known artifacts become cards with a title/description; markdown reports have their body
///   expanded inline.
/// - Extra .md/.html not on the list are appended afterward as plain cards (to avoid omissions).
pub fn refresh_index() {
    let dir = report_dir();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Table-of-contents entries: known artifacts (only those that exist) + extra unknown .md/.html.
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
    entries.extend(extras.into_iter().map(|f| (f.clone(), f, "(auto-collected)".to_string())));

    let mut cards = String::new();
    for (file, title, desc) in &entries {
        let path = dir.join(file);
        let stamp = mtime_epoch(&path).map(format_kst).unwrap_or_default();
        let body = if file.ends_with(".md") {
            let md = std::fs::read_to_string(&path).unwrap_or_default();
            format!(
                "<details open><summary>report body</summary><div class=\"md\">{}</div></details>",
                render_markdown_html(&md)
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
<title>supragnosis e2e report</title>
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
  .md {{ font-size: 13px; }}
  .md h1 {{ font-size: 14px; margin: 10px 0 4px; color: #c3cad4; }}
  .md h2 {{ font-size: 13px; margin: 12px 0 4px; color: #c3cad4; }}
  .md p {{ margin: 4px 0; }}
  .md ul {{ margin: 4px 0; padding-left: 20px; }}
  .md li {{ margin: 1px 0; }}
  .md code {{ background: #101216; border: 1px solid #262b33; border-radius: 4px;
             padding: 0 4px; font-size: 12px; }}
  .md table {{ border-collapse: collapse; margin: 6px 0; display: block;
              overflow-x: auto; font-variant-numeric: tabular-nums; }}
  .md th, .md td {{ border: 1px solid #2a2f38; padding: 4px 9px; font-size: 12.5px;
                   text-align: left; white-space: nowrap; }}
  .md th {{ background: #1d212a; color: #c3cad4; }}
  .md tr:nth-child(even) td {{ background: #14171d; }}
</style>
<h1>supragnosis e2e report</h1>
<p class="sub">Table of contents for the real-model end-to-end measurement suite's artifacts - generated {stamp}.
Runs of the `cargo test -p supragnosis-e2e -- --ignored` family refresh this page.</p>
{cards}"##,
        stamp = format_kst(now),
    );
    std::fs::write(dir.join("index.html"), html).expect("write index");
}
