//! The main window: the app's own pages and the caller check their commands run, the turns
//! between the graph, Review, Settings and Setup, and the window's kept frame and text size.

use super::*;

/// The app's own pages: bundled with it, never served by a daemon or a hub, and the only pages its
/// commands answer - Settings (docs/settings-page.md) and Welcome (docs/onboarding.md Section 6).
const OWN_PAGES: &[&str] = &["settings.html", "welcome.html"];

/// The address of one of the app's own pages.
fn own_page(name: &str) -> tauri::Url {
    format!("tauri://localhost/{name}").parse().expect("static url")
}

pub(crate) fn settings_url() -> tauri::Url {
    own_page("settings.html")
}

/// Whether `url` is the viewer's own address - its scheme and host, `viz://localhost`. Only such an
/// address is a way back:
/// anything else a page could put in `?from=` would make "< Graph" a redirect (settings-page.md
/// Section 4, S1; design-system.md Section 29.4).
pub(crate) fn is_viewer(url: &tauri::Url) -> bool {
    let own = graph_url();
    url.scheme() == own.scheme() && url.host_str() == own.host_str()
}

/// The graph as the person left it (design-system.md Section 29.4): the address the window's page
/// carries in `?from=` when it is an app page and that address is the viewer's own; the viewer's
/// address itself, `#review` dropped, when the window shows the viewer; else `None`.
pub(crate) fn graph_left(current: &tauri::Url) -> Option<tauri::Url> {
    let mut back = if is_viewer(current) {
        current.clone()
    } else {
        let from = current.query_pairs().find(|(k, _)| k == "from")?.1.into_owned();
        tauri::Url::parse(&from).ok().filter(is_viewer)?
    };
    back.set_fragment(None);
    Some(back)
}

/// An app page's address carrying the way back to the graph, when there is one.
pub(crate) fn with_from(mut page: tauri::Url, current: Option<&tauri::Url>) -> tauri::Url {
    if let Some(back) = current.and_then(graph_left) {
        page.query_pairs_mut().append_pair("from", back.as_str());
    }
    page
}

pub(crate) fn welcome_url() -> tauri::Url {
    own_page("welcome.html")
}

/// Whether `url` is the app's own page `name`: its scheme, host and path.
fn is_page(url: &tauri::Url, name: &str) -> bool {
    let own = own_page(name);
    url.scheme() == own.scheme() && url.host_str() == own.host_str() && url.path() == own.path()
}

/// Whether a caller may run the app's commands (docs/settings-page.md S1, docs/onboarding.md
/// Section 6): the main window, while its page is one of the app's own. The capability cannot tell
/// the pages of one window apart, since the same window shows the viewer a daemon or a hub serves,
/// so this check is the lock. It reads the webview's top-level address, so a viewer page that frames
/// an app page cannot call through the frame either.
pub(crate) fn own_page_ok(label: &str, url: &tauri::Url) -> bool {
    label == "main" && OWN_PAGES.iter().any(|p| is_page(url, p))
}

pub(crate) fn own_page_caller(webview: &tauri::Webview) -> Result<(), String> {
    let url = webview.url().map_err(|e| e.to_string())?;
    if own_page_ok(webview.label(), &url) {
        Ok(())
    } else {
        tracing::warn!(label = webview.label(), page = %url, "an app command was refused: not one of the app's own pages");
        Err("only the app's own pages - Settings and Welcome - may change a setting (docs/settings-page.md S1)".into())
    }
}

/// Reloads the viewer if that is what the main window shows - not the settings page, which re-reads
/// its own state after every change.
pub(crate) fn reload_viewer(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.url().is_ok_and(|u| u.scheme() == "viz") {
            let _ = w.eval("location.reload()");
        }
    }
}

/// The root's text size the View menu sets (design-system.md Section 10, U21), kept in view.json.
/// Every rem size of the viewer and the app's pages follows the root; the webview itself is not
/// zoomed, so the title bar keeps its height and the traffic lights their place.
pub(crate) struct TextScale(pub(crate) Mutex<f64>);

/// A text size held to 100%-200% in steps of 12.5%.
fn clamp_text_scale(scale: f64) -> f64 {
    (scale.clamp(1.0, 2.0) * 8.0).round() / 8.0
}

/// The next text size: a step up (+1), a step down (-1), or Actual Size (0).
pub(crate) fn step_text_scale(scale: f64, step: i32) -> f64 {
    if step == 0 {
        return 1.0;
    }
    clamp_text_scale(scale + 0.125 * f64::from(step))
}

/// The script that gives a page the root's text size and tells the viewer's map to read it again.
pub(crate) fn text_scale_script(scale: f64) -> String {
    format!(
        "document.documentElement.style.fontSize = \"{}%\"; window.dispatchEvent(new Event(\"rootsize\"));",
        scale * 100.0
    )
}

fn view_prefs_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("view.json"))
}

/// Keeps one of the view preferences - the text's size, the window's frame - beside the others, so
/// writing one never drops another (design-system.md Section 29.14).
fn keep_view_pref(
    app: &tauri::AppHandle,
    key: &str,
    value: serde_json::Value,
) -> std::io::Result<()> {
    let Some(path) = view_prefs_path(app) else {
        return Ok(());
    };
    let mut all = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    all[key] = value;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Replaced whole, through a temporary file, so a write cut short loses neither preference.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, all.to_string())?;
    std::fs::rename(&tmp, &path)
}

/// The window's kept frame - its place and size in logical points - when it is on a screen now.
fn kept_frame(app: &tauri::AppHandle) -> Option<WindowFrame> {
    let v = view_prefs_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())?;
    let f = &v["window"];
    let mut frame = WindowFrame {
        x: f["x"].as_f64()?,
        y: f["y"].as_f64()?,
        w: f["w"].as_f64()?.max(MIN_WINDOW.0),
        h: f["h"].as_f64()?.max(MIN_WINDOW.1),
    };
    let screens: Vec<(f64, f64, f64, f64)> = app
        .available_monitors()
        .ok()?
        .iter()
        .map(|m| {
            let s = m.scale_factor();
            let (p, z) = (m.position(), m.size());
            (
                f64::from(p.x) / s,
                f64::from(p.y) / s,
                f64::from(z.width) / s,
                f64::from(z.height) / s,
            )
        })
        .collect();
    // Its size is cut to the screen it opens on (design-system.md Section 29.14).
    let (_, _, sw, sh) = screens
        .iter()
        .copied()
        .find(|s| frame_on_a_screen(&frame, std::slice::from_ref(s)))?;
    frame.w = frame.w.min(sw).max(MIN_WINDOW.0);
    frame.h = frame.h.min(sh).max(MIN_WINDOW.1);
    Some(frame)
}

/// The window's frame in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowFrame {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) w: f64,
    pub(crate) h: f64,
}

/// The smallest window: 760 pixels keeps the Graph in its wide layout, so the narrow one stays the
/// browser's (design-system.md Section 29.14).
pub(crate) const MIN_WINDOW: (f64, f64) = (760.0, 480.0);

/// Whether a kept frame's title bar is on one of the screens now, so the window can be found and
/// moved; a place on no screen is forgotten (design-system.md Section 29.14).
pub(crate) fn frame_on_a_screen(f: &WindowFrame, screens: &[(f64, f64, f64, f64)]) -> bool {
    let (cx, cy) = (f.x + f.w / 2.0, f.y + 12.0);
    screens
        .iter()
        .any(|&(x, y, w, h)| cx >= x && cx <= x + w && cy >= y && cy <= y + h)
}

/// The frame the window has now, kept in memory as it moves and resizes, written when it closes or
/// the app quits.
pub(crate) struct FrameNow(pub(crate) Mutex<Option<WindowFrame>>);

pub(crate) fn note_frame(w: &tauri::Window) {
    let (Ok(p), Ok(z), Ok(s)) = (w.outer_position(), w.inner_size(), w.scale_factor()) else {
        return;
    };
    if w.is_fullscreen().unwrap_or(false) || w.is_minimized().unwrap_or(false) {
        return; // a full-screen or minimized window keeps the frame it had before
    }
    if let Some(now) = w.app_handle().try_state::<FrameNow>() {
        *now.0.lock().unwrap() = Some(WindowFrame {
            x: f64::from(p.x) / s,
            y: f64::from(p.y) / s,
            w: f64::from(z.width) / s,
            h: f64::from(z.height) / s,
        });
    }
}

pub(crate) fn keep_frame(app: &tauri::AppHandle) {
    let Some(f) = app.try_state::<FrameNow>().and_then(|n| *n.0.lock().unwrap()) else {
        return;
    };
    let v = serde_json::json!({ "x": f.x, "y": f.y, "w": f.w, "h": f.h });
    if let Err(e) = keep_view_pref(app, "window", v) {
        tracing::warn!(error = %e, "the window's frame was not kept");
    }
}

/// The kept text size; 100% when the file is missing or unreadable.
pub(crate) fn read_text_scale(app: &tauri::AppHandle) -> f64 {
    view_prefs_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["text_scale"].as_f64())
        .map_or(1.0, clamp_text_scale)
}

/// Steps the text size, keeps it, and gives it to the page on screen.
pub(crate) fn set_text_scale(app: &tauri::AppHandle, step: i32) -> tauri::Result<()> {
    let Some(state) = app.try_state::<TextScale>() else {
        return Ok(());
    };
    let scale = {
        let mut s = state.0.lock().unwrap();
        *s = step_text_scale(*s, step);
        *s
    };
    if let Err(e) = keep_view_pref(app, "text_scale", serde_json::json!(scale)) {
        tracing::warn!(error = %e, "the text size was not kept");
    }
    if let Some(w) = app.get_webview_window("main") {
        w.eval(text_scale_script(scale))?;
    }
    Ok(())
}

/// Turns the main window to the Welcome page, creating the window first if needed.
pub(crate) fn show_welcome(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let current = w.url().ok();
        if !current.as_ref().is_some_and(|u| is_page(u, "welcome.html")) {
            w.navigate(with_from(welcome_url(), current.as_ref()))?;
        }
    }
    Ok(())
}

/// The graph's address: the viewer, which the shell's `viz://` proxy serves.
pub(crate) fn graph_url() -> tauri::Url {
    "viz://localhost/".parse().expect("static url")
}

/// Turns the main window to the settings page (docs/settings-page.md Section 3.0), creating the
/// window first if the app has retreated to the tray. With a section, opens that section; without
/// one, a window already on the settings page stays where it is.
pub(crate) fn show_settings(app: &tauri::AppHandle, section: Option<&str>) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        let current = w.url().ok();
        let there = current.as_ref().is_some_and(|u| is_page(u, "settings.html"));
        if !there || section.is_some() {
            let mut url = with_from(settings_url(), current.as_ref());
            url.set_fragment(section);
            w.navigate(url)?;
        }
    }
    Ok(())
}

/// Turns the main window to the Review page (docs/design-system.md Section 7): a route of the viewer's
/// own page, `#review`. From the settings page that is a navigation; on the viewer it is the hash, so
/// the graph's state is kept.
pub(crate) fn show_review(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if let Some(u) = w.url().ok().filter(|u| own_page_ok("main", u)) {
            let mut url = graph_left(&u).unwrap_or_else(graph_url);
            url.set_fragment(Some("review"));
            w.navigate(url)?;
        } else {
            w.eval("location.hash = '#review'")?;
        }
    }
    Ok(())
}

/// The tray's Review item opens the page at All workspaces, the scope its count is of
/// (docs/design-system.md Section 7). The menu bar's Review keeps the scope on screen.
pub(crate) fn show_review_all(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if w.url().is_ok_and(|u| own_page_ok("main", &u)) {
            let mut url = graph_url();
            url.query_pairs_mut().append_pair("workspace", "*");
            url.set_fragment(Some("review"));
            w.navigate(url)?;
        } else {
            w.eval("typeof openReviewAt === 'function' ? openReviewAt('*') : (location.hash = '#review')")?;
        }
    }
    Ok(())
}

/// Turns the main window to the graph, creating it first if needed.
pub(crate) fn show_graph(app: &tauri::AppHandle) -> tauri::Result<()> {
    show_viewer(app)?;
    if let Some(w) = app.get_webview_window("main") {
        if let Some(u) = w.url().ok().filter(|u| own_page_ok("main", u)) {
            // Back to the graph as the person left it (design-system.md Section 29.4).
            w.navigate(graph_left(&u).unwrap_or_else(graph_url))?;
        }
    }
    Ok(())
}

/// Shows the viewer window (creating it on first use / after the app retreated to the tray) and
/// returns the app to the dock. Closing the window hides it and drops back to Accessory (menu
/// bar only) - see on_window_event.
pub(crate) fn show_viewer(app: &tauri::AppHandle) -> tauri::Result<()> {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let builder = WebviewWindowBuilder::new(
        app,
        "main",
        WebviewUrl::External("viz://localhost/".parse().expect("static url")),
    )
    .title(&app.package_info().name)
    .min_inner_size(MIN_WINDOW.0, MIN_WINDOW.1)
    .initialization_script(include_str!("../assets/shell-init.js"))
    // Each page the window loads - the viewer, Settings, Setup - takes the kept text size (U21).
    .on_page_load(|w, payload| {
        if payload.event() == tauri::webview::PageLoadEvent::Finished {
            if let Some(state) = w.app_handle().try_state::<TextScale>() {
                let scale = *state.0.lock().unwrap();
                if scale != 1.0 {
                    let _ = w.eval(text_scale_script(scale));
                }
            }
        }
    });
    // Merge the title bar into the viewer header: the macOS title bar becomes a transparent
    // overlay (traffic lights float over the content, no title text), and shell-init.js turns
    // the header into the drag region with its left edge padded clear of the lights.
    // The lights are pinned to the header row's geometry (headerHeight in the shell-page-loaded
    // log line; 49px today, center 24.5). NOTE tao's semantics (macos/view.rs
    // inset_traffic_lights): y is NOT the button's top - tao grows the titlebar container to
    // (button height + y) and the button keeps its default in-container offset, so the visual
    // button center lands slightly BELOW y (center = y - 1.5 as calibrated on macOS 15).
    // y=26 centers on 24.5, confirmed visually; recalibrate if headerHeight changes (x=14
    // mirrors the header's own padding, and tao re-applies the inset on every redraw, so it
    // survives window events). Because the container is sized off the BUTTON's height, the
    // mapping also depends on the traffic-light metrics, and macOS 26 gates those on the
    // linked SDK - an app built with an older SDK gets the compatibility metrics and lands
    // the lights off-center (seen on the macos-14-built release under macOS 26). The release
    // build pins an SDK-26 runner for the app; build locally with a current Xcode. y=26
    // re-confirmed on macOS 26 with the SDK-26 build.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(14.0, 26.0));
    // The window opens where the person left it, at its size, when that place is on a screen now;
    // otherwise at the default size, centred (design-system.md Section 29.14).
    let builder = match kept_frame(app) {
        Some(f) => builder.inner_size(f.w, f.h).position(f.x, f.y),
        None => builder.inner_size(1280.0, 860.0).center(),
    };
    builder.build()?;
    Ok(())
}
