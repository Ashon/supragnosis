//! The menu bar's map commands: which of them act on the page on screen, and the scripts that
//! press the page's own controls or send it its own keys.

use super::*;

/// The menu bar's map commands, by id, so each page's load report can enable the ones that act there.
pub(crate) struct MapItems(pub(crate) Vec<(&'static str, MenuItem<tauri::Wry>)>);

/// What a press of one of the menu bar's own items does (design-system.md Section 29.13).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MenuPress {
    Settings,
    Setup,
    Graph,
    Review,
    /// A step of the text size: up (1), down (-1), or Actual Size (0).
    Text(i32),
    Docs,
    Issue,
    /// Done where they report their outcome, on This Mac.
    Diagnostics,
    ShowLog,
    /// One of the map's commands, which `map_command` does.
    Map,
}

/// The menu bar's own items - id, words, key and press - which the menu is built from and its
/// presses are read through. None of the ids is the tray's, so a press does only its own act.
pub(crate) const MENU_ITEMS: &[(&str, &str, Option<&str>, MenuPress)] = &[
    ("settings", "Settings...", Some("CmdOrCtrl+,"), MenuPress::Settings),
    ("setup", "Setup...", None, MenuPress::Setup),
    ("graph", "Graph", Some("CmdOrCtrl+1"), MenuPress::Graph),
    ("review", "Review", Some("CmdOrCtrl+2"), MenuPress::Review),
    ("view-filters", "Filters Panel", Some("Ctrl+CmdOrCtrl+S"), MenuPress::Map),
    ("view-log", "Log Panel", Some("Ctrl+CmdOrCtrl+L"), MenuPress::Map),
    ("view-map", "as Map", None, MenuPress::Map),
    ("view-list", "as List", None, MenuPress::Map),
    ("view-fit", "Fit to View", None, MenuPress::Map),
    ("view-keys", "Keys...", None, MenuPress::Map),
    ("view-back", "Back", Some("CmdOrCtrl+["), MenuPress::Map),
    ("view-forward", "Forward", Some("CmdOrCtrl+]"), MenuPress::Map),
    ("text-bigger", "Make Text Bigger", Some("CmdOrCtrl+="), MenuPress::Text(1)),
    ("text-smaller", "Make Text Smaller", Some("CmdOrCtrl+-"), MenuPress::Text(-1)),
    ("text-actual", "Actual Size", Some("CmdOrCtrl+0"), MenuPress::Text(0)),
    ("edit-find", "Find", Some("CmdOrCtrl+F"), MenuPress::Map),
    ("help-docs", "Supragnosis Documentation", None, MenuPress::Docs),
    ("help-diagnostics", "Diagnostics...", None, MenuPress::Diagnostics),
    ("help-log", "Show Log in Finder", None, MenuPress::ShowLog),
    ("help-issue", "Report an Issue...", None, MenuPress::Issue),
];

/// The menu bar item `id` names; `None` for any other id, the tray's among them.
pub(crate) fn menu_item(
    id: &str,
) -> Option<(&'static str, &'static str, Option<&'static str>, MenuPress)> {
    MENU_ITEMS.iter().find(|m| m.0 == id).copied()
}

/// The controls the map commands press, which each page reports it has when it loads (shell-init.js
/// is given this list), so the shell enables the items that act there.
pub(crate) const MAP_CONTROLS: &[&str] =
    &["panelL", "panelR", "mapTab", "listBtn", "fit", "keysOpen", "keysDlg"];

/// Which map commands act on a page, from its address and the controls it reported (design-system.md
/// Section 29.13): on the Graph the ones whose control it has; on Review, Settings and Setup the ones
/// that open the Graph, Back, and Forward on the Graph only; nothing on the starting page or a viewer
/// without the selection history.
pub(crate) fn map_items_enabled(url: &str, controls: &[String]) -> Vec<(&'static str, bool)> {
    let has = |c: &str| controls.iter().any(|x| x == c);
    let viewer = url.starts_with("viz://");
    let app_page = url.contains("settings.html") || url.contains("welcome.html");
    let review = viewer && url.contains("#review");
    let graph = viewer && !review;
    let current = has("keysDlg");
    let opens = |control: &str| app_page || review || (graph && has(control));
    vec![
        ("view-filters", opens("panelL")),
        ("view-log", opens("panelR")),
        ("view-map", opens("mapTab")),
        ("view-list", opens("listBtn")),
        ("view-fit", opens("fit")),
        ("view-keys", opens("keysOpen")),
        ("edit-find", app_page || viewer),
        ("view-back", app_page || (viewer && current)),
        ("view-forward", graph && current),
    ]
}

pub(crate) fn enable_map_items(app: &tauri::AppHandle, report: &serde_json::Value) {
    let url = report["url"].as_str().unwrap_or("");
    let controls: Vec<String> = report["controls"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    if let Some(items) = app.try_state::<MapItems>() {
        for (id, on) in map_items_enabled(url, &controls) {
            if let Some((_, item)) = items.0.iter().find(|(k, _)| *k == id) {
                let _ = item.set_enabled(on);
            }
        }
    }
}

/// The script a map command runs in the viewer (design-system.md Section 29.13): it presses the page's
/// own control, only when the press changes something and no dialog or card is open, so a command and
/// its button do one thing.
pub(crate) fn press_js(id: &str, only_if_unselected: bool) -> String {
    let id = serde_json::to_string(id).unwrap_or_default();
    format!(
        r##"(function(){{if(document.querySelector("dialog[open]")||document.querySelector("#obscard.on"))return;var b=document.getElementById({id});if(!b||b.getAttribute("aria-disabled")==="true")return;{guard}b.click();}})();"##,
        guard = if only_if_unselected {
            r#"if(b.getAttribute("aria-selected")==="true")return;"#
        } else {
            ""
        }
    )
}

/// The script that sends the page on screen a Cmd key (Cmd+K, Cmd+[ or Cmd+]) at its focused element,
/// which each page reads as it reads the key; a viewer without the selection history gets no Cmd+[.
pub(crate) fn key_js(key: &str, needs_history: bool) -> String {
    let k = serde_json::to_string(key).unwrap_or_default();
    format!(
        r#"(function(){{{gate}var t=document.activeElement||document.body;t.dispatchEvent(new KeyboardEvent("keydown",{{key:{k},metaKey:true,bubbles:true,cancelable:true}}));}})();"#,
        gate = if needs_history {
            r#"if(/^viz:/.test(location.href)&&!document.getElementById("keysDlg"))return;"#
        } else {
            ""
        }
    )
}

/// The menu bar's map commands (design-system.md Section 29.13): on the Graph page they press the
/// page's own control; on Review, Settings or Setup they open the Graph and do nothing else; Back and
/// Forward send the page on screen its own Cmd+[ and Cmd+].
pub(crate) fn map_command(app: &tauri::AppHandle, id: &str) -> tauri::Result<()> {
    let control = match id {
        "view-filters" => Some(("panelL", false)),
        "view-log" => Some(("panelR", false)),
        "view-map" => Some(("mapTab", true)),
        "view-list" => Some(("listBtn", true)),
        "view-fit" => Some(("fit", false)),
        "view-keys" => Some(("keysOpen", false)),
        "edit-find" | "view-back" | "view-forward" => None,
        _ => return Ok(()),
    };
    let Some(w) = app.get_webview_window("main") else {
        return Ok(());
    };
    if id == "view-back" || id == "view-forward" {
        return w.eval(key_js(if id == "view-back" { "[" } else { "]" }, true));
    }
    let url = w.url().ok();
    let viewer = url.as_ref().is_some_and(is_viewer);
    if !viewer {
        return show_graph(app);
    }
    if url.as_ref().is_some_and(|u| u.fragment() == Some("review")) {
        return w.eval("location.hash = ''");
    }
    match control {
        Some((c, tab)) => w.eval(press_js(c, tab)),
        None => w.eval(key_js("k", false)),
    }
}

/// Opens Settings' This Mac with an act the page reports itself (Section 29.5): `diagnostics` focuses
/// Copy Diagnostics, `reveal-log` shows the log in Finder.
pub(crate) fn show_settings_act(app: &tauri::AppHandle, act: &str) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let current = w.url().ok();
        let mut url = with_from(settings_url(), current.as_ref());
        url.query_pairs_mut().append_pair("act", act);
        url.set_fragment(Some("daemon"));
        w.navigate(url)?;
    }
    Ok(())
}

/// Opens an address of the project's in the browser; nothing of this Mac's goes with it.
pub(crate) fn open_url(url: &str) -> tauri::Result<()> {
    if let Err(e) = Command::new("open").arg(url).status() {
        tracing::error!(error = %e, url, "could not open the address");
    }
    Ok(())
}
