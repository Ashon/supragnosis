//! Embed the viewer assets, minifying them in release builds only.
//!
//! The source of truth stays the readable, lintable files in `assets/` (ESLint runs on those). At
//! build time each is copied to `OUT_DIR` - verbatim in debug (fast, debuggable), minified in release.
//! `lib.rs` embeds from `OUT_DIR` via `include_str!`, so the crate is still a single self-contained
//! binary and the build is still pure cargo (these are Rust build-dependencies - no Node).
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

    for name in ["viewer.html", "viewer.css", "viewer.js"] {
        let src = assets.join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        let content = fs::read_to_string(&src)
            .unwrap_or_else(|e| panic!("read {}: {e}", src.display()));
        let processed = if release {
            match name {
                "viewer.css" => minify_css(&content),
                "viewer.js" => minify_js(&content),
                "viewer.html" => minify_html_doc(&content),
                _ => content,
            }
        } else {
            content
        };
        fs::write(out.join(name), processed)
            .unwrap_or_else(|e| panic!("write {name} to OUT_DIR: {e}"));
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
