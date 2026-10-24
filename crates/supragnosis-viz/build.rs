//! Embed the viewer assets, minifying them in release builds only.
//!
//! The source of truth stays the readable, lintable files in `assets/` (ESLint runs on those). At
//! build time each is copied to `OUT_DIR` - verbatim in debug (fast, debuggable), minified in release.
//! `lib.rs` embeds from `OUT_DIR` via `include_str!`, so the crate is still a single self-contained
//! binary and the build is still pure cargo (these are Rust build-dependencies - no Node).
//!
//! `viewer.js` is the one asset kept in parts: `join_viewer` joins `assets/viewer/*.js` into it,
//! and the joined source is also written unminified to `OUT_DIR/viewer.src.js`, the text the
//! tests read.
//!
//! Minification is conservative on JS: oxc's code generator re-prints the parsed AST with whitespace
//! and comments removed, but performs NO identifier mangling or dead-code elimination (that is
//! `oxc_minifier`, which is deliberately not used) - so it is semantics-preserving printing. Parsing
//! also fails the build on malformed JS, a free correctness check. CSS uses lightningcss, HTML uses
//! minify-html.

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let release = env::var("PROFILE").as_deref() == Ok("release");

    let assets = Path::new(&manifest).join("assets");
    let out = Path::new(&out_dir);

    let viewer_js = join_viewer(&assets.join("viewer"));
    fs::write(out.join("viewer.src.js"), &viewer_js).expect("write viewer.src.js to OUT_DIR");

    for name in ["viewer.html", "viewer.css", "viewer.js", "viewer-sim.js", "viewer-gl.js"] {
        let content = if name == "viewer.js" {
            viewer_js.clone()
        } else {
            let src = assets.join(name);
            println!("cargo:rerun-if-changed={}", src.display());
            fs::read_to_string(&src).unwrap_or_else(|e| panic!("read {}: {e}", src.display()))
        };
        let processed = if release {
            match name {
                "viewer.css" => minify_css(&content),
                "viewer.js" | "viewer-sim.js" | "viewer-gl.js" => minify_js(&content),
                "viewer.html" => minify_html_doc(&content),
                _ => content,
            }
        } else {
            content
        };
        fs::write(out.join(name), processed)
            .unwrap_or_else(|e| panic!("write {name} to OUT_DIR: {e}"));
    }

    // The faces the viewer ships (docs/design-system.md Section 4, U11): every file under
    // assets/fonts, served at /fonts/<its path there> with its bytes compiled in, so the binary stays
    // the only thing to install and no page fetches a font from another host.
    let fonts = assets.join("fonts");
    let mut files = Vec::new();
    walk(&fonts, &mut files);
    files.sort();
    let mut table = String::from("pub(crate) static FONTS: &[(&str, &str, &[u8])] = &[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        let rel = f
            .strip_prefix(&fonts)
            .expect("under assets/fonts")
            .to_string_lossy()
            .replace('\\', "/");
        let kind = match f.extension().and_then(|e| e.to_str()) {
            Some("woff2") => "font/woff2",
            Some("css") => "text/css; charset=utf-8",
            _ => "text/plain; charset=utf-8",
        };
        table.push_str(&format!(
            "    (\"/fonts/{rel}\", \"{kind}\", include_bytes!({:?})),\n",
            f.display().to_string()
        ));
    }
    table.push_str("];\n");
    fs::write(out.join("fonts.rs"), table).expect("write fonts.rs to OUT_DIR");

    // The marks the viewer draws (docs/design-system.md Section 28, U41): Lucide's files, unmodified,
    // served at /icons/lucide-<release>/<name>.svg. The release is read from each file's own license
    // line, so a file from another release fails the build instead of being served under this one's
    // path, which is cached for a year.
    let icons = assets.join("icons");
    println!("cargo:rerun-if-changed={}", icons.display());
    let mut svgs: Vec<_> = fs::read_dir(&icons)
        .unwrap_or_else(|e| panic!("read {}: {e}", icons.display()))
        .map(|e| e.expect("a directory entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("svg"))
        .collect();
    svgs.sort();
    let mut release: Option<String> = None;
    let mut table = String::from("pub(crate) static ICONS: &[(&str, &str, &[u8])] = &[\n");
    for f in &svgs {
        println!("cargo:rerun-if-changed={}", f.display());
        let text = fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
        let this = text
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("<!-- @license lucide-static v"))
            .and_then(|l| l.strip_suffix(" - ISC -->"))
            .unwrap_or_else(|| panic!("{} does not start with Lucide's license line", f.display()))
            .to_string();
        match &release {
            None => release = Some(this),
            Some(r) if *r == this => {}
            Some(r) => panic!("{} is Lucide {this}, the others {r}", f.display()),
        }
        let name = f.file_name().expect("a file name").to_string_lossy();
        table.push_str(&format!(
            "    (\"/icons/lucide-{}/{name}\", \"image/svg+xml\", include_bytes!({:?})),\n",
            release.as_deref().expect("set above"),
            f.display().to_string()
        ));
    }
    table.push_str("];\n");
    fs::write(out.join("icons.rs"), table).expect("write icons.rs to OUT_DIR");
}

/// The viewer script, served at `/viewer.js`, kept as ordered parts in `assets/viewer/` and joined
/// here in file-name order. It is a classic script, so its top-level declarations share one scope
/// whichever part holds them, and each part is cut between two top-level statements and adds
/// nothing of its own (no header, no separator): the joined text is the one file the parts were
/// cut from, byte for byte. The two-digit prefix is the order:
///
/// - `01-state.js` - the type band and palette, the canvases, List, the page's shared state
/// - `02-frame.js` - the layout's last report, level of detail, render scheduling, the camera,
///   type colors
/// - `03-graph.js` - the remembered layout, the counts, applying a graph, the sync pill
/// - `04-controls.js` - controls that say what they are, Settings, the legend
/// - `05-trust.js` - trust and contest on the map, the poll, workspaces, the empty map
/// - `06-notices.js` - node lookup, `esc` and `html`, icons, notices, the glossary, curation
/// - `07-review.js` - the Review page and its Show on the map
/// - `08-log.js` - the observation log, the strip, the event stream, peer markers
/// - `09-inspector.js` - settling a contested value, the detail inspector, the person's place
/// - `10-minimap.js` - the minimap, fitting the view, workspace outlines and boundaries
/// - `11-regions.js` - region clouds, the scene grid, semantic zoom, the layout's host
/// - `12-draw.js` - haloed labels and the frame's draw
/// - `13-input.js` - pointer, touch, search, the view menu, keys, the panels, startup
///
/// A function may be called from any part, since nothing runs it before the script has loaded, but
/// code that runs at load reads only what an earlier part or line declared. ESLint reads the joined
/// text too (`npm run lint` in `assets/`), so `no-undef` still sees the one scope.
fn join_viewer(dir: &Path) -> String {
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut parts: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|e| e.expect("a directory entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("js"))
        .collect();
    parts.sort();
    assert!(!parts.is_empty(), "{} holds no part of the viewer script", dir.display());
    let mut js = String::new();
    for p in &parts {
        println!("cargo:rerun-if-changed={}", p.display());
        js.push_str(&fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display())));
    }
    js
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn minify_css(src: &str) -> String {
    use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
    let mut sheet = StyleSheet::parse(src, ParserOptions::default())
        .unwrap_or_else(|e| panic!("viewer.css parse: {e:?}"));
    sheet
        .minify(MinifyOptions::default())
        .unwrap_or_else(|e| panic!("viewer.css minify: {e:?}"));
    sheet
        .to_css(PrinterOptions { minify: true, ..Default::default() })
        .unwrap_or_else(|e| panic!("viewer.css print: {e:?}"))
        .code
}

fn minify_html_doc(src: &str) -> String {
    use minify_html::{minify, Cfg};
    // Our HTML links external css/js, so leave those engines off; just collapse markup whitespace.
    let cfg = Cfg { minify_css: false, minify_js: false, ..Cfg::default() };
    String::from_utf8(minify(src.as_bytes(), &cfg)).expect("viewer.html is utf-8 after minify")
}

fn minify_js(src: &str) -> String {
    use oxc::allocator::Allocator;
    use oxc::codegen::{Codegen, CodegenOptions, CommentOptions};
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let allocator = Allocator::default();
    // A classic browser script ("use strict";), not an ES module - parse in script mode.
    let source_type = SourceType::cjs();
    let ret = Parser::new(&allocator, src, source_type).parse();
    assert!(
        ret.diagnostics.is_empty(),
        "viewer.js failed to parse during minify (fix the source): {:?}",
        ret.diagnostics
    );
    // minify: compact printing; comments:false drops the (heavy) comments. No mangling, no
    // compression - this is semantics-preserving printing, not oxc_minifier.
    Codegen::new()
        .with_options(CodegenOptions {
            minify: true,
            comments: CommentOptions::disabled(),
            ..Default::default()
        })
        .build(&ret.program)
        .code
}
