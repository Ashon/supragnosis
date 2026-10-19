//! docs/design-system.md Sections 6 and 6a, U26: the title bar is a Mac toolbar. It names the place on
//! screen, never the product; its leading end holds the filters toggle and the title, its trailing end
//! the search and the right panel's toggle; and every page but the Graph goes back with "< Graph",
//! Esc and Cmd+[. The author chose this over the first title bar on 2026-10-19.

const HTML: &str = include_str!("../assets/viewer.html");
const SCRIPT: &str = include_str!("../assets/viewer.js");
const CSS: &str = include_str!("../assets/viewer.css");
const SETTINGS_HTML: &str = include_str!("../../../app/assets/settings.html");
const WELCOME_HTML: &str = include_str!("../../../app/assets/welcome.html");
const SETTINGS_JS: &str = include_str!("../../../app/assets/settings.js");
const KIT: &str = include_str!("../../../app/assets/kit.js");

/// A page's `<header>`, comments dropped.
fn header(html: &str) -> String {
    let at = html.find("<header>").expect("a header");
    let block = &html[at..at + html[at..].find("</header>").expect("it closes")];
    let mut clean = String::new();
    let mut rest = block;
    while let Some(i) = rest.find("<!--") {
        clean.push_str(&rest[..i]);
        rest = rest[i..].find("-->").map_or("", |j| &rest[i + j + 3..]);
    }
    clean.push_str(rest);
    clean
}

fn function(name: &str) -> &'static str {
    let at = SCRIPT
        .find(&format!("function {name}("))
        .unwrap_or_else(|| panic!("no function {name}"));
    &SCRIPT[at..at + SCRIPT[at..].find("\n}\n").expect("it closes")]
}

/// U26 on the viewer: the filters toggle, the way back and the place's title lead; the search and the
/// right panel's toggle trail; the product's name is not the title; the Review page's own heading and
/// link moved into the bar; and Cmd+[ goes back.
#[test]
fn the_viewer_title_bar_is_a_mac_toolbar() {
    let bar = header(HTML);
    let at =
        |needle: &str| bar.find(needle).unwrap_or_else(|| panic!("the title bar has no {needle}"));
    let order = [
        at("id=\"panelL\""),
        at("id=\"backGraph\""),
        at("id=\"placeTitle\""),
        at("class=\"searchwrap\""),
        at("id=\"panelR\""),
    ];
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "the title bar is not leading toggle, title, search, toggle"
    );
    assert!(!bar.contains("</svg>supragnosis"), "the title bar names the product");
    let place = function("renderPlace");
    for part in [
        "placeTitle",
        "statsScope(currentWs())",
        "\"Review\"",
        "backGraph",
        "panelL",
        "panelR",
    ] {
        assert!(place.contains(part), "the title bar does not follow the place: {part}");
    }
    let review = &HTML[HTML.find("id=\"reviewPage\"").expect("the Review page")..];
    let review = &review[..review.find("</section>").unwrap()];
    assert!(
        !review.contains("<h2>Review</h2>") && !review.contains("rv-close"),
        "the Review page keeps its own heading"
    );
    assert!(
        SCRIPT.contains("e.key === \"[\" && (e.metaKey || e.ctrlKey)"),
        "Cmd+[ does not go back"
    );
    let flat: String = CSS.split_whitespace().collect();
    assert!(
        flat.contains("#placeTitle{display:none;}"),
        "a phone's title does not give its room to the search"
    );
    assert!(
        flat.contains(".tback[hidden]{display:none;}"),
        "the way back shows on the Graph page"
    );
}

/// U26 on the app's pages: "< Graph" where the crumb and the x were, the page's own title, and Esc and
/// Cmd+[ go back; Settings titles itself by the pane on screen.
#[test]
fn the_app_pages_go_back_to_the_graph() {
    for (file, html, title) in
        [("settings.html", SETTINGS_HTML, ""), ("welcome.html", WELCOME_HTML, "Setup")]
    {
        let bar = header(html);
        assert!(
            bar.contains("id=\"close\" class=\"tback\"") && bar.contains(">Graph</a>"),
            "{file}: no way back named Graph"
        );
        assert!(
            !bar.contains("crumb") && !bar.contains("i-x") && !bar.contains("</svg>supragnosis"),
            "{file}: the crumb or the x remains"
        );
        assert!(
            bar.contains(&format!("<h1 id=\"pageTitle\">{title}</h1>")),
            "{file}: the title is not the page's"
        );
        assert!(
            bar.find("id=\"close\"") < bar.find("id=\"pageTitle\""),
            "{file}: the way back does not lead"
        );
    }
    assert!(
        SETTINGS_JS.contains("document.getElementById(\"pageTitle\").textContent ="),
        "Settings is not titled by its pane"
    );
    assert!(
        KIT.contains("e.key === \"[\" && (e.metaKey || e.ctrlKey)"),
        "Cmd+[ does not go back on the app's pages"
    );
}
