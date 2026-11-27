//! The menu bar's map commands: which of them act on the page on screen, and the scripts that
//! press the page's own controls or send it its own keys.

use super::*;

/// The menu bar's map commands, by id, so each page's load report can enable the ones that act there.
pub(crate) struct MapItems(pub(crate) Vec<(&'static str, MenuItem<Rt>)>);

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

/// The menu bar's own items - id, the key of its words in app/assets/words.json, key and press -
/// which the menu is built from and its presses are read through. None of the ids is the tray's, so a
/// press does only its own act.
pub(crate) const MENU_ITEMS: &[(&str, &str, Option<&str>, MenuPress)] = &[
    ("settings", "menu.settings", Some("CmdOrCtrl+,"), MenuPress::Settings),
    ("setup", "menu.setup", None, MenuPress::Setup),
    ("graph", "page.graph", Some("CmdOrCtrl+1"), MenuPress::Graph),
    ("review", "menu.review", Some("CmdOrCtrl+2"), MenuPress::Review),
    ("view-filters", "menu.filtersPanel", Some("Ctrl+CmdOrCtrl+S"), MenuPress::Map),
    ("view-log", "menu.logPanel", Some("Ctrl+CmdOrCtrl+L"), MenuPress::Map),
    ("view-map", "menu.asMap", None, MenuPress::Map),
    ("view-list", "menu.asList", None, MenuPress::Map),
    ("view-fit", "menu.fit", None, MenuPress::Map),
    ("view-keys", "menu.keys", None, MenuPress::Map),
    ("view-back", "menu.back", Some("CmdOrCtrl+["), MenuPress::Map),
    ("view-forward", "menu.forward", Some("CmdOrCtrl+]"), MenuPress::Map),
    ("text-bigger", "menu.bigger", Some("CmdOrCtrl+="), MenuPress::Text(1)),
    ("text-smaller", "menu.smaller", Some("CmdOrCtrl+-"), MenuPress::Text(-1)),
    ("text-actual", "menu.actualSize", Some("CmdOrCtrl+0"), MenuPress::Text(0)),
    ("edit-find", "menu.find", Some("CmdOrCtrl+F"), MenuPress::Map),
    ("help-docs", "menu.docs", None, MenuPress::Docs),
    ("help-diagnostics", "menu.diagnostics", None, MenuPress::Diagnostics),
    ("help-log", "menu.showLog", None, MenuPress::ShowLog),
    ("help-issue", "menu.issue", None, MenuPress::Issue),
];

/// One place in the menu bar: one of [`MENU_ITEMS`] by id, one of the platform's items macOS acts
/// on with the key of its words, or a separator.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Entry {
    Item(&'static str),
    Platform(Platform, &'static str),
    Separator,
}

/// The platform's items the menu bar holds, which macOS acts on itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Platform {
    About,
    Services,
    Hide,
    HideOthers,
    Quit,
    CloseWindow,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    FullScreen,
    Minimize,
    Zoom,
}

use Entry::{Item, Separator};
use Platform as P;

/// The menu bar, menu by menu: its id, the key of its title (none for the app menu, which macOS
/// titles with the app's name) and its places (design-system.md Section 29.13). It is built from the
/// words rather than from `Menu::default`, whose titles and items are English, so the bar speaks the
/// app's language (docs/i18n.md Section 8 step 3); the Window and Help menus keep the ids that make
/// them the menus macOS lists windows in and puts its help search in.
pub(crate) const MENU_BAR: &[(&str, &str, &[Entry])] = &[
    (
        "menu-app",
        "",
        &[
            Entry::Platform(P::About, "menu.about"),
            Separator,
            Item("settings"),
            Item("setup"),
            Separator,
            Entry::Platform(P::Services, "menu.services"),
            Separator,
            Entry::Platform(P::Hide, "menu.hide"),
            Entry::Platform(P::HideOthers, "menu.hideOthers"),
            Separator,
            // Its words follow the tray's Quit (D15); apply_tray sets them again.
            Entry::Platform(P::Quit, "menu.quit"),
        ],
    ),
    ("menu-file", "menu.file", &[Entry::Platform(P::CloseWindow, "menu.closeWindow")]),
    (
        "menu-edit",
        "menu.edit",
        &[
            Entry::Platform(P::Undo, "menu.undo"),
            Entry::Platform(P::Redo, "menu.redo"),
            Separator,
            Entry::Platform(P::Cut, "menu.cut"),
            Entry::Platform(P::Copy, "menu.copy"),
            Entry::Platform(P::Paste, "menu.paste"),
            Entry::Platform(P::SelectAll, "menu.selectAll"),
            Separator,
            Item("edit-find"),
        ],
    ),
    (
        "menu-view",
        "menu.view",
        &[
            Item("graph"),
            Item("review"),
            Separator,
            Item("view-filters"),
            Item("view-log"),
            Item("view-map"),
            Item("view-list"),
            Item("view-fit"),
            Item("view-keys"),
            Separator,
            Item("view-back"),
            Item("view-forward"),
            Separator,
            // Text that can grow (design-system.md Section 10, U21): the root's size, not the
            // webview's zoom, so the title bar and its traffic lights stay put.
            Item("text-bigger"),
            Item("text-smaller"),
            Item("text-actual"),
            Separator,
            Entry::Platform(P::FullScreen, "menu.fullScreen"),
        ],
    ),
    (
        tauri::menu::WINDOW_SUBMENU_ID,
        "menu.window",
        &[
            Entry::Platform(P::Minimize, "menu.minimize"),
            Entry::Platform(P::Zoom, "menu.zoom"),
            Separator,
            Entry::Platform(P::CloseWindow, "menu.closeWindow"),
        ],
    ),
    (
        tauri::menu::HELP_SUBMENU_ID,
        "menu.help",
        &[
            Item("help-docs"),
            Separator,
            Item("help-diagnostics"),
            Item("help-log"),
            Separator,
            Item("help-issue"),
        ],
    ),
];

/// The menu bar built from [`MENU_BAR`] in the app's words, the map's commands kept so each page's
/// load report can say which of them act there, and the app menu's Quit, whose words follow the
/// tray's (D15).
pub(crate) fn menu_bar(app: &tauri::AppHandle<Rt>) -> tauri::Result<(Menu<Rt>, MapItems, AppQuit)> {
    let info = app.package_info();
    let name = info.name.clone();
    let about = tauri::menu::AboutMetadata {
        name: Some(name.clone()),
        version: Some(info.version.to_string()),
        copyright: app.config().bundle.copyright.clone(),
        authors: app.config().bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };
    let bar = Menu::new(app)?;
    let (mut map, mut quit) = (Vec::new(), None);
    for (id, title, places) in MENU_BAR {
        let title = if title.is_empty() { name.clone() } else { word(title) };
        let sub = tauri::menu::Submenu::with_id(app, *id, title, true)?;
        for place in *places {
            match *place {
                Separator => sub.append(&PredefinedMenuItem::separator(app)?)?,
                Item(id) => {
                    let (id, key, accel, press) = menu_item(id).expect("an item of MENU_ITEMS");
                    let it = MenuItem::with_id(app, id, word(key), true, accel)?;
                    sub.append(&it)?;
                    if press == MenuPress::Map {
                        map.push((id, it));
                    }
                }
                Entry::Platform(p, key) => {
                    let words = word_with(key, &[("name", &name)]);
                    let t = Some(words.as_str());
                    let it = match p {
                        P::About => PredefinedMenuItem::about(app, t, Some(about.clone()))?,
                        P::Services => PredefinedMenuItem::services(app, t)?,
                        P::Hide => PredefinedMenuItem::hide(app, t)?,
                        P::HideOthers => PredefinedMenuItem::hide_others(app, t)?,
                        P::Quit => PredefinedMenuItem::quit(app, t)?,
                        P::CloseWindow => PredefinedMenuItem::close_window(app, t)?,
                        P::Undo => PredefinedMenuItem::undo(app, t)?,
                        P::Redo => PredefinedMenuItem::redo(app, t)?,
                        P::Cut => PredefinedMenuItem::cut(app, t)?,
                        P::Copy => PredefinedMenuItem::copy(app, t)?,
                        P::Paste => PredefinedMenuItem::paste(app, t)?,
                        P::SelectAll => PredefinedMenuItem::select_all(app, t)?,
                        P::FullScreen => PredefinedMenuItem::fullscreen(app, t)?,
                        P::Minimize => PredefinedMenuItem::minimize(app, t)?,
                        P::Zoom => PredefinedMenuItem::maximize(app, t)?,
                    };
                    if p == P::Quit {
                        quit = Some(it.clone());
                    }
                    sub.append(&it)?;
                }
            }
        }
        bar.append(&sub)?;
    }
    Ok((bar, MapItems(map), AppQuit(quit.expect("the app menu holds Quit"))))
}

/// What a press of the menu bar's item `id` does; any other id, the tray's or a platform item, does
/// nothing here. `open` opens one of the project's addresses in the browser.
pub(crate) fn menu_pressed(
    app: &tauri::AppHandle<Rt>,
    id: &str,
    open: impl Fn(&str) -> tauri::Result<()>,
) {
    let Some((_, _, _, press)) = menu_item(id) else {
        return;
    };
    let shown = match press {
        MenuPress::Settings => show_settings(app, None),
        MenuPress::Setup => show_welcome(app),
        MenuPress::Graph => show_graph(app),
        MenuPress::Review => show_review(app),
        MenuPress::Text(step) => set_text_scale(app, step),
        MenuPress::Docs => open(&format!("{}#readme", env!("CARGO_PKG_REPOSITORY"))),
        MenuPress::Issue => open(&format!("{}/issues", env!("CARGO_PKG_REPOSITORY"))),
        // Done where they report their outcome, on This Mac (design-system.md 29.13).
        MenuPress::Diagnostics => show_settings_act(app, "diagnostics"),
        MenuPress::ShowLog => show_settings_act(app, "reveal-log"),
        MenuPress::Map => map_command(app, id),
    };
    if let Err(e) = shown {
        tracing::error!(error = %e, "failed to turn the main window");
    }
}

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

pub(crate) fn enable_map_items(app: &tauri::AppHandle<Rt>, report: &serde_json::Value) {
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
pub(crate) fn map_command(app: &tauri::AppHandle<Rt>, id: &str) -> tauri::Result<()> {
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
pub(crate) fn show_settings_act(app: &tauri::AppHandle<Rt>, act: &str) -> tauri::Result<()> {
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
