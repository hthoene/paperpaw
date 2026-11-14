//! The gallery of one small plugin on one device: its pictures, pages and
//! contact sheet are written, and every link on the pages leads to a file.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::path::Path;

use paperpaw_dev::cli::Context;
use paperpaw_dev::gallery;
use paperpaw_dev::report::Report;
use paperpaw_dev::target::Target;
use paperpaw_dev::variant::Variant;

/// The links (`href`, `src`) of an HTML page, without anchors.
fn links(html: &str) -> Vec<String> {
    let mut links = Vec::new();
    for attribute in ["href=\"", "src=\""] {
        let mut rest = html;
        while let Some(start) = rest.find(attribute) {
            rest = &rest[start + attribute.len()..];
            let end = rest.find('"').unwrap();
            let link = rest[..end].split('#').next().unwrap().replace("&amp;", "&");
            if !link.is_empty() {
                links.push(link);
            }
            rest = &rest[end..];
        }
    }
    links
}

fn pages(dir: &Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            pages(&path, found);
        } else if path.extension().is_some_and(|e| e == "html") {
            found.push(path);
        }
    }
}

#[test]
#[ignore = "the dev tool measures a widget's cell with its interface-6 probe widget, which the native Home does not place (todo 634, #244)"]
fn a_plugin_s_gallery_is_written_and_linked() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-gallery-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec!["widgets".into()],
            ..Target::default()
        },
        out.clone(),
    );
    context.profiles = vec![Variant::x4().profile];
    context.quick = true;
    let mut report = Report::default();
    gallery::run(&context, &mut report).unwrap();

    let dir = out.join("gallery");
    assert!(
        report
            .pages
            .iter()
            .any(|p| p.path == Path::new("gallery/index.html"))
    );
    let index = std::fs::read_to_string(dir.join("index.html")).unwrap();
    let index_links = links(&index);
    assert!(
        index_links.contains(&"widgets/xteink-x4.png".to_string()),
        "{index_links:?}"
    );
    assert!(dir.join("widgets/xteink-x4.png").is_file());
    // A page per widget, with its sizes' pictures.
    let widget_page = index_links
        .iter()
        .find(|l| l.starts_with("widgets/widgets/") && l.ends_with(".html"))
        .expect("a widget page");
    let widget = std::fs::read_to_string(dir.join(widget_page)).unwrap();
    assert!(widget.contains("1x1.png"), "{widget}");

    let mut found = Vec::new();
    pages(&dir, &mut found);
    assert!(found.len() >= 3, "{found:?}");
    for page in found {
        let html = std::fs::read_to_string(&page).unwrap();
        for link in links(&html) {
            let target = page.parent().unwrap().join(&link);
            assert!(target.is_file(), "{} links {link}", page.display());
        }
    }
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn the_ui_kit_shows_every_page() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-ui-kit-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec![paperpaw_dev::ui_kit::PLUGIN.into()],
            add: Some(paperpaw_dev::ui_kit::add),
            ..Target::default()
        },
        out.clone(),
    );
    context.profiles = vec![Variant::x4().profile];
    context.quick = true;
    let mut report = Report::default();
    gallery::run(&context, &mut report).unwrap();
    let screens = out.join("gallery/ui-kit/screens");
    for page in [
        "text",
        "messages",
        "empties",
        "rows",
        "values",
        "noted-rows",
        "numbers",
        "sliders",
        "controls",
        "segments",
        "layout",
        "share",
        "offer",
        "dialog",
        "inputs",
        "ticks",
        "chips",
        "pickers",
        "choice",
        "confirm",
    ] {
        let page = screens.join(format!("ui-kit.{page}.html"));
        assert!(page.is_file(), "{}", page.display());
    }
    let _ = std::fs::remove_dir_all(&out);
}

/// Long German labels and the pseudo-language at the largest interface
/// text on the X4: no block of the kit shortens a label (an error with
/// `ui-kit`), none reaches past the screen.
#[test]
fn the_ui_kit_keeps_every_label_whole_at_the_largest_text() {
    let out =
        std::env::temp_dir().join(format!("paperpaw-dev-ui-kit-whole-{}", std::process::id()));
    let mut context = Context::new(
        Target {
            plugins: vec![paperpaw_dev::ui_kit::PLUGIN.into()],
            add: Some(paperpaw_dev::ui_kit::add),
            ..Target::default()
        },
        out.clone(),
    );
    context.profiles = vec![Variant::x4().profile];
    context.languages = Some(vec!["de".into(), "pseudo".into()]);
    context.nights = vec![false];
    context.themes = paperpaw_dev::variant::THEMES.iter().take(1).collect();
    context.settings = vec![("core.ui-size".into(), "24".into())];
    context.labels_whole = true;
    let mut report = Report::default();
    paperpaw_dev::visual::run(&context, &mut report).unwrap();
    let _ = std::fs::remove_dir_all(&out);
    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.level == paperpaw_dev::report::Level::Error)
        .map(|f| format!("{} {}: {}", f.check, f.place, f.message))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
    // It looked: the kit's pages were drawn and checked.
    assert!(report.ran.iter().any(|c| c == "visual") || !report.findings.is_empty());
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.place.starts_with("ui-kit.")),
        "nothing of the kit was looked at"
    );
}
