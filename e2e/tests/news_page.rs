//! The news page (docs/design-system.md Section 11b): the notes in `docs/news` and the newest
//! releases' notes in `docs/releases`, with the dates and lines CHANGELOG.md's table gives, written
//! into `site/news` and held to it. The site keeps no build step, so the pages are committed and this
//! test fails when one differs from what the sources make; `SUPRAGNOSIS_WRITE_PAGE=1` rewrites them
//! and removes a page whose item is gone.

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const ORIGIN: &str = "https://supragnosis.dev/";
const REPO: &str = "https://github.com/Ashon/supragnosis";
/// Releases with a page of their own; the older ones are in the changelog.
const RELEASES: usize = 12;
/// Items beside an item's page.
const RECENT: usize = 8;
/// Items in the feed.
const FEED: usize = 20;
const UPDATE: &str = "brew upgrade\nsupragnosis restart";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Note,
    Release,
}

struct Item {
    kind: Kind,
    slug: String,
    title: String,
    summary: String,
    /// `YYYY-MM-DD`.
    date: String,
    version: Option<String>,
    /// The file the text comes from, from the repository's root.
    source: String,
    body: String,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository")
        .to_path_buf()
}

fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `key: value` lines between `---` fences, and the text after them.
fn front_matter(text: &str, file: &str) -> (BTreeMap<String, String>, String) {
    let rest = text.strip_prefix("---\n").unwrap_or_else(|| panic!("{file}: no front matter"));
    let (head, body) = rest.split_once("\n---\n").unwrap_or_else(|| panic!("{file}: unclosed"));
    let data = head
        .lines()
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    (data, body.to_string())
}

/// Every item, newest first: a note before a release of the same day.
fn items() -> Vec<Item> {
    let mut notes = Vec::new();
    for entry in std::fs::read_dir(root().join("docs/news")).expect("docs/news") {
        let path = entry.expect("an entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let slug = path.file_stem().and_then(|s| s.to_str()).expect("a name").to_string();
        let source = format!("docs/news/{slug}.md");
        let (data, body) = front_matter(&read(&source), &source);
        let field = |k: &str| data.get(k).cloned().unwrap_or_else(|| panic!("{source}: no {k}"));
        notes.push(Item {
            kind: Kind::Note,
            slug,
            title: field("title"),
            summary: field("summary"),
            date: field("date"),
            version: data.get("version").cloned(),
            source,
            body,
        });
    }
    // CHANGELOG.md's table: `| [vX](docs/releases/vX.md) | YYYY-MM-DD | the line |`, newest first.
    let changelog = read("CHANGELOG.md");
    let releases = changelog
        .lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.split(" | ").collect();
            let tag = cells.first()?.strip_prefix("| [v")?.split(']').next()?.to_string();
            let date = cells.get(1)?.trim().to_string();
            let line = cells.get(2)?.trim().trim_end_matches('|').trim().to_string();
            Some((tag, date, line))
        })
        .take(RELEASES)
        .map(|(tag, date, line)| {
            let source = format!("docs/releases/v{tag}.md");
            Item {
                kind: Kind::Release,
                slug: format!("v{tag}"),
                title: format!("supragnosis v{tag}"),
                summary: line,
                date,
                version: Some(tag),
                body: read(&source),
                source,
            }
        });
    let mut all: Vec<Item> = notes.into_iter().chain(releases).collect();
    all.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| (a.kind == Kind::Release).cmp(&(b.kind == Kind::Release)))
    });
    all
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A one-line summary as HTML: escaped, with its `code` spans set as code.
fn inline(s: &str) -> String {
    esc(s)
        .split('`')
        .enumerate()
        .map(|(i, part)| if i % 2 == 1 { format!("<code>{part}</code>") } else { part.to_string() })
        .collect()
}

/// `Oct 28, 2026` from `2026-10-28`.
fn shown(date: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut parts = date.split('-').map(|p| p.parse::<usize>().unwrap_or(0));
    let (y, m, d) = (parts.next(), parts.next(), parts.next());
    match (y, m, d) {
        (Some(y), Some(m @ 1..=12), Some(d)) => format!("{} {d}, {y}", MONTHS[m - 1]),
        _ => date.to_string(),
    }
}

/// The feed's date, RFC 822, at midnight UTC.
fn rfc822(date: &str) -> String {
    let mut parts = date.split('-').map(|p| p.parse::<i64>().unwrap_or(0));
    let (y, m, d) =
        (parts.next().unwrap_or(0), parts.next().unwrap_or(1), parts.next().unwrap_or(1));
    // Sakamoto's day of the week.
    let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let yy = if m < 3 { y - 1 } else { y };
    let wd = (yy + yy / 4 - yy / 100 + yy / 400 + t[(m - 1) as usize] + d) % 7;
    let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let month = shown(date).split(' ').next().unwrap_or("Jan").to_string();
    format!("{}, {d:02} {month} {y} 00:00:00 +0000", days[wd as usize])
}

/// A link in a note's text: a web address or an anchor as written; anything else is a path from
/// the file it is in, and points at that file on GitHub.
fn link_to(dest: &str, source: &str) -> String {
    if dest.starts_with("http://")
        || dest.starts_with("https://")
        || dest.starts_with("mailto:")
        || dest.starts_with('#')
    {
        return dest.to_string();
    }
    let (path, anchor) = dest.split_once('#').map_or((dest, ""), |(p, a)| (p, a));
    let mut parts: Vec<&str> = source.split('/').collect();
    parts.pop();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let anchor = if anchor.is_empty() { String::new() } else { format!("#{anchor}") };
    format!("{REPO}/blob/main/{}{anchor}", parts.join("/"))
}

/// A note's markdown as HTML. Raw HTML in it is shown as text, and links follow [`link_to`].
fn markdown(md: &str, source: &str) -> String {
    let events = Parser::new_ext(md, Options::ENABLE_TABLES).map(|e| match e {
        Event::Html(h) | Event::InlineHtml(h) => Event::Text(h),
        Event::Start(Tag::Link { link_type, dest_url, title, id }) => Event::Start(Tag::Link {
            link_type,
            dest_url: CowStr::from(link_to(&dest_url, source)),
            title,
            id,
        }),
        other => other,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

fn kind_word(k: Kind) -> &'static str {
    match k {
        Kind::Note => "Note",
        Kind::Release => "Release",
    }
}

/// The kind, the version and the date of an item, as the list and its page show them.
fn meta(item: &Item) -> String {
    let version = item.version.as_deref().map(|v| format!("<span>v{}</span>", esc(v)));
    format!(
        "<p class=\"n-meta\"><span class=\"n-kind\">{}</span>{}<time datetime=\"{}\">{}</time></p>",
        kind_word(item.kind),
        if item.kind == Kind::Note {
            version.unwrap_or_default()
        } else {
            String::new()
        },
        esc(&item.date),
        shown(&item.date)
    )
}

/// A whole page: the site's head, header and footer around `main`, `depth` folders below the root.
fn page(depth: usize, title: &str, description: &str, path: &str, main: &str) -> String {
    let up = "../".repeat(depth);
    let (title, description) = (esc(title), esc(description));
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <meta name="description" content="{description}">
  <link rel="canonical" href="{ORIGIN}{path}">
  <meta name="theme-color" content="#08090d">
  <meta property="og:title" content="{title}">
  <meta property="og:description" content="{description}">
  <meta property="og:type" content="article">
  <meta property="og:url" content="{ORIGIN}{path}">
  <meta property="og:site_name" content="supragnosis">
  <meta property="og:image" content="{ORIGIN}assets/og.png">
  <meta name="twitter:card" content="summary_large_image">
  <link rel="alternate" type="application/rss+xml" title="supragnosis news" href="{ORIGIN}news/feed.xml">
  <link rel="icon" type="image/svg+xml" href="{up}favicon.svg">
  <link rel="preload" href="{up}fonts/fraunces-5.3.0/fraunces-latin-opsz-normal.woff2" as="font" type="font/woff2" crossorigin>
  <link rel="stylesheet" href="{up}fonts/pretendard-1.3.9/pretendard-latin.css">
  <link rel="stylesheet" href="{up}style.css">
  <!-- Visits are counted, as the Privacy page says; on supragnosis.dev only. -->
  <script src="{up}analytics.js" async></script>
</head>
<body>
  <a class="skip" href="#main">Skip to content</a>

  <header class="site-head wrap">
    <a class="wordmark" href="{up}">supragnosis</a>
    <nav aria-label="Sections">
      <a href="{up}#how">How it works</a>
      <a href="{up}principles/">Principles</a>
      <a href="{up}#status">Status</a>
      <a href="{up}news/" aria-current="page">News</a>
      <a href="{REPO}#readme">Docs</a>
      <a href="{REPO}">GitHub</a>
    </nav>
  </header>

  <!-- Written by e2e/tests/news_page.rs from docs/news, docs/releases and CHANGELOG.md
       (docs/design-system.md Section 11b); edit those, not this page. -->
  <main id="main">
{main}
  </main>

  <footer class="site-foot wrap">
    <p class="eyebrow">supra <span>(above)</span> + gnosis <span>(knowing)</span> = meta-knowledge</p>
    <nav aria-label="Footer">
      <a href="{REPO}">GitHub</a>
      <a href="{up}news/">News</a>
      <a href="{up}principles/">Principles</a>
      <a href="{up}privacy/">Privacy</a>
      <a href="{REPO}/blob/main/CHANGELOG.md">Changelog</a>
    </nav>
    <p class="fine">MIT or Apache-2.0. Plain HTML and CSS with a few scripts, no build step. This page lives in <code>site/news/</code>.</p>
  </footer>
</body>
</html>
"##
    )
}

fn index(items: &[Item]) -> String {
    let latest = items.iter().find(|i| i.kind == Kind::Note);
    let card = latest.map_or(String::new(), |n| {
        format!(
            "    <section class=\"wrap n-latest\" aria-label=\"The latest note\">\n      <a class=\"n-card\" href=\"{}/\">\n        {}\n        <h2>{}</h2>\n        <p>{}</p>\n      </a>\n    </section>\n",
            esc(&n.slug),
            meta(n),
            esc(&n.title),
            inline(&n.summary)
        )
    });
    let rows: String = items
        .iter()
        .map(|i| {
            format!(
                "        <li><a href=\"{}/\"><time datetime=\"{}\">{}</time><span class=\"n-kind\">{}</span><span class=\"n-title\">{}</span><span class=\"n-sum\">{}</span></a></li>\n",
                esc(&i.slug),
                esc(&i.date),
                shown(&i.date),
                kind_word(i.kind),
                esc(&i.title),
                inline(&i.summary)
            )
        })
        .collect();
    let main = format!(
        "    <section class=\"n-hero wrap\">\n      <p class=\"eyebrow\">News</p>\n      <h1>What changed, and what we learned changing it.</h1>\n      <p class=\"sub\">Release notes, and now and then a longer note from the work. Updating is <code>brew upgrade</code>, then <code>supragnosis restart</code>.</p>\n      <p class=\"n-feed\"><a href=\"feed.xml\">Follow with RSS</a></p>\n    </section>\n\n{card}\n    <section class=\"wrap n-all\" aria-label=\"Everything, newest first\">\n      <ol class=\"n-list\">\n{rows}      </ol>\n      <p class=\"n-older\"><a href=\"{REPO}/blob/main/CHANGELOG.md\">Older releases are in the changelog</a></p>\n    </section>"
    );
    page(1, "News - supragnosis", "Release notes for supragnosis, and longer notes on what was measured and learned while making it.", "news/", &main)
}

fn item_page(items: &[Item], at: usize) -> String {
    let item = &items[at];
    let recent: String = items
        .iter()
        .take(RECENT)
        .map(|i| {
            let current = if i.slug == item.slug { " aria-current=\"page\"" } else { "" };
            format!(
                "          <li><a href=\"../{}/\"{current}>{}<time datetime=\"{}\">{}</time></a></li>\n",
                esc(&i.slug),
                esc(&i.title),
                esc(&i.date),
                shown(&i.date)
            )
        })
        .collect();
    let step = |i: Option<&Item>, word: &str| {
        i.map_or(String::new(), |i| {
            format!("<a href=\"../{}/\"><span>{word}</span>{}</a>", esc(&i.slug), esc(&i.title))
        })
    };
    let newer = step(at.checked_sub(1).map(|n| &items[n]), "Newer");
    let older = step(items.get(at + 1), "Older");
    let main = format!(
        "    <div class=\"wrap n-page\">\n      <aside class=\"n-recent\" aria-label=\"Recent news\">\n        <p class=\"eyebrow\">Recent</p>\n        <ol>\n{recent}        </ol>\n        <p><a href=\"../feed.xml\">Follow with RSS</a></p>\n      </aside>\n      <article class=\"n-article\">\n        <p class=\"n-crumb\"><a href=\"../\">News</a></p>\n        {}\n        <h1>{}</h1>\n        <p class=\"n-lede\">{}</p>\n        <div class=\"n-body\">\n{}        </div>\n        <div class=\"n-update\">\n          <p class=\"eyebrow\">Update</p>\n          <pre><code>{}</code></pre>\n        </div>\n        <nav class=\"n-steps\" aria-label=\"Newer and older\">{newer}{older}</nav>\n      </article>\n    </div>",
        meta(item),
        esc(&item.title),
        inline(&item.summary),
        markdown(&item.body, &item.source),
        esc(UPDATE)
    );
    page(
        2,
        &format!("{} - supragnosis", item.title),
        &item.summary.replace('`', ""),
        &format!("news/{}/", item.slug),
        &main,
    )
}

fn feed(items: &[Item]) -> String {
    let entries: String = items
        .iter()
        .take(FEED)
        .map(|i| {
            let link = format!("{ORIGIN}news/{}/", i.slug);
            format!(
                "    <item>\n      <title>{}</title>\n      <link>{link}</link>\n      <guid>{link}</guid>\n      <pubDate>{}</pubDate>\n      <description>{}</description>\n    </item>\n",
                esc(&i.title),
                rfc822(&i.date),
                esc(&i.summary.replace('`', ""))
            )
        })
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<rss version=\"2.0\">\n  <channel>\n    <title>supragnosis news</title>\n    <link>{ORIGIN}news/</link>\n    <description>Release notes for supragnosis, and longer notes on what was measured and learned while making it.</description>\n    <language>en</language>\n{entries}  </channel>\n</rss>\n"
    )
}

/// Every file under `site/news` the sources make, by its path from `site/news`.
fn pages() -> BTreeMap<String, String> {
    let items = items();
    let mut out = BTreeMap::new();
    out.insert("index.html".to_string(), index(&items));
    out.insert("feed.xml".to_string(), feed(&items));
    for (at, item) in items.iter().enumerate() {
        out.insert(format!("{}/index.html", item.slug), item_page(&items, at));
    }
    out
}

/// docs/design-system.md Section 11b: the committed news pages are what the notes, the releases'
/// notes and the changelog make, and no page is left whose item is gone.
#[test]
fn the_news_pages_are_what_their_sources_make() {
    let dir = root().join("site/news");
    let made = pages();
    let write = std::env::var("SUPRAGNOSIS_WRITE_PAGE").as_deref() == Ok("1");
    let on_disk: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(|n| format!("{n}/index.html")))
                .collect()
        })
        .unwrap_or_default();
    if write {
        for gone in on_disk.iter().filter(|p| !made.contains_key(*p)) {
            let folder = dir.join(gone.trim_end_matches("/index.html"));
            std::fs::remove_dir_all(&folder).expect("remove a page whose item is gone");
        }
        for (path, text) in &made {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().expect("a folder")).expect("its folder");
            std::fs::write(&file, text).expect("write the page");
        }
        return;
    }
    let mut stale: Vec<String> = made
        .iter()
        .filter(|(path, text)| std::fs::read_to_string(dir.join(path)).ok().as_ref() != Some(*text))
        .map(|(path, _)| path.clone())
        .collect();
    stale.extend(on_disk.into_iter().filter(|p| !made.contains_key(p)));
    assert!(
        stale.is_empty(),
        "site/news differs from what docs/news, docs/releases and CHANGELOG.md make: {stale:?}. \
         Run with SUPRAGNOSIS_WRITE_PAGE=1 to rewrite it, and review the diff"
    );
}

/// The list holds every note and the newest releases, newest first, with the latest note above it;
/// each item has a page and is in the feed.
#[test]
fn the_list_holds_every_note_and_the_newest_releases() {
    let items = items();
    let notes = items.iter().filter(|i| i.kind == Kind::Note).count();
    let releases = items.iter().filter(|i| i.kind == Kind::Release).count();
    assert_eq!(releases, RELEASES);
    assert!(notes >= 1, "a note is published");
    assert!(items.windows(2).all(|w| w[0].date >= w[1].date), "newest first");
    let made = pages();
    let index = &made["index.html"];
    for i in &items {
        assert!(made.contains_key(&format!("{}/index.html", i.slug)), "{} has a page", i.slug);
        assert!(index.contains(&format!("href=\"{}/\"", i.slug)), "{} is listed", i.slug);
    }
    let first_note = items.iter().find(|i| i.kind == Kind::Note).expect("a note");
    let card = index.find("n-card").expect("the latest note's card");
    assert!(index[card..].starts_with(&format!("n-card\" href=\"{}/\"", first_note.slug)));
    assert!(
        index.find("n-card").unwrap() < index.find("n-list").unwrap(),
        "the card is above the list"
    );
}

/// A note's text: raw HTML is shown as text, and a path into the repository points at GitHub.
#[test]
fn a_note_is_text_and_its_paths_point_at_github() {
    let html = markdown(
        "A <script>x</script> and [the design](../recall.md#7-measuring-recall).",
        "docs/news/n.md",
    );
    assert!(!html.contains("<script>"), "{html}");
    assert!(html.contains("&lt;script&gt;"), "{html}");
    assert!(
        html.contains(&format!("href=\"{REPO}/blob/main/docs/recall.md#7-measuring-recall\"")),
        "{html}"
    );
    assert_eq!(link_to("https://x.example/", "docs/news/n.md"), "https://x.example/");
    assert_eq!(rfc822("2026-10-28"), "Wed, 28 Oct 2026 00:00:00 +0000");
    assert_eq!(shown("2026-10-28"), "Oct 28, 2026");
}
