//! docs/design-system.md U19, the part that needs no screen change: the viewer's controls say what
//! they are. View's toggles are switches whose state is set with their class, the legend's chips say
//! whether they are pressed, the workspace chips are one radio group, the page declares its language
//! and the map's canvas is named. The rest of U19 - dialogs, keys, targets, reflow - is owed until
//! Section 14 steps 13-16 meet it, and its registry row says so.

const SCRIPT: &str = include_str!("../assets/viewer.js");
const PAGE: &str = include_str!("../assets/viewer.html");

/// The id of every `<button ... class="tog ...">` in the page.
fn toggles() -> Vec<&'static str> {
    PAGE.match_indices("<button id=\"")
        .filter_map(|(at, _)| {
            let tag = &PAGE[at..at + PAGE[at..].find('>').expect("the tag closes")];
            let id = tag[12..].split('"').next()?;
            tag.contains("class=\"tog").then_some(id)
        })
        .collect()
}

#[test]
fn the_viewers_controls_say_what_they_are() {
    let togs = toggles();
    assert!(togs.len() >= 10, "the View toggles were read ({togs:?})");
    // Each switch's state is set where its class is, never the class alone.
    assert!(
        SCRIPT.contains("for (const b of document.querySelectorAll(\"button.tog\")) setSwitch(b, b.classList.contains(\"on\"));"),
        "the switches do not say their state when the page starts"
    );
    for id in &togs {
        for change in ["toggle", "add", "remove"] {
            assert!(
                !SCRIPT.contains(&format!("{id}.classList.{change}(\"on\"")),
                "{id} changes its class without saying its state"
            );
        }
    }
    assert!(
        !SCRIPT.contains("e.currentTarget.classList.toggle(\"on\""),
        "a View toggle changes its class without saying its state"
    );
    let switch = &SCRIPT[SCRIPT.find("function setSwitch(").expect("setSwitch")..];
    let switch = &switch[..switch.find("\n}\n").expect("it closes")];
    for part in ["\"role\", \"switch\"", "\"aria-checked\"", "classList.toggle(\"on\""] {
        assert!(switch.contains(part), "setSwitch does not set {part}");
    }

    // A legend chip filters, so it says whether it is pressed; the workspace chips choose one scope.
    assert!(
        SCRIPT.contains("setPressed(el, !offSet.has(t));"),
        "a legend chip does not say it is pressed"
    );
    assert!(
        SCRIPT.matches("radioGroup(chipBar, \"Workspace\")").count() >= 2,
        "the workspace chips are not one radio group"
    );
    let radios = &SCRIPT[SCRIPT.find("function radioGroup(").expect("radioGroup")..];
    let radios = &radios[..radios.find("\n}\n").expect("it closes")];
    for part in [
        "\"radiogroup\"",
        "\"radio\"",
        "\"aria-checked\"",
        "tabIndex",
        "ArrowRight",
        "Home",
        "End",
    ] {
        assert!(radios.contains(part), "radioGroup does not handle {part}");
    }

    // The page says its language, and the map is named.
    assert!(PAGE.contains("<html lang=\"en\">"), "the viewer does not declare its language");
    assert!(
        PAGE.contains("<canvas id=\"c\" role=\"img\" aria-label=\""),
        "the map's canvas has no name"
    );
}
