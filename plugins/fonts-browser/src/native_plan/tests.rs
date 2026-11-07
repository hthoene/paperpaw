use super::*;
use crate::{fail_alloc::sweep, native_catalog::FontFile};
use alloc::{string::ToString, vec};

const SHA1: &str = "0123456789abcdef0123456789abcdef01234567";

fn file(path: &str, size: u64) -> FontFile {
    FontFile {
        path: path.to_string(),
        url: "https://f.example/x".to_string(),
        size,
        sha1: SHA1.to_string(),
    }
}

fn font(files: Vec<FontFile>) -> Font {
    Font {
        id: "fredoka".to_string(),
        name: "Fredoka".to_string(),
        version: "1.0".to_string(),
        files,
        ..Font::default()
    }
}

#[test]
fn a_font_is_planned_into_its_folder() {
    let font = font(vec![
        file("plugin.ini", 100),
        file("fonts/regular.ppfp", 2000),
    ]);
    let plan = Plan::new(&font, false).unwrap().unwrap();
    assert_eq!(plan.plugin, "font-fredoka");
    assert_eq!(plan.folder, "staging/font-fredoka");
    assert_eq!(
        plan.steps[1].save,
        "staging/font-fredoka/fonts/regular.ppfp"
    );
    assert_eq!(plan.size(), 2100);
    assert_eq!(plan.max_file, MAX_PACK);
    assert_eq!(
        Plan::new(&font, true).unwrap().unwrap().max_file,
        MAX_OUTLINE
    );
}

#[test]
fn what_cannot_be_installed_has_no_plan() {
    let ok = || vec![file("plugin.ini", 1)];
    let mut cases: Vec<Font> = Vec::new();
    cases.push(font(Vec::new()));
    cases.push(font(vec![file("theme.ini", 1)]));
    let mut f = ok();
    f.push(file("../x", 1));
    cases.push(font(f));
    let mut f = ok();
    f.push(file("plugin.ini", 2));
    cases.push(font(f));
    let mut f = ok();
    f[0].sha1 = "z".repeat(40);
    cases.push(font(f));
    let mut f = ok();
    f[0].sha1 = SHA1[..39].to_string();
    cases.push(font(f));
    let mut f = ok();
    f[0].size = 0;
    cases.push(font(f));
    let mut f = ok();
    f[0].size = MAX_PACK + 1;
    cases.push(font(f));
    let mut f = ok();
    f[0].url = "a\nb".to_string();
    cases.push(font(f));
    let mut f = ok();
    f[0].url = "u".repeat(MAX_URL + 1);
    cases.push(font(f));
    let mut f = font(ok());
    f.id = "Bad Id".to_string();
    // A bad id never gets here from the catalog, but a plan checks it.
    cases.push(f);
    cases.push(font(
        (0..=MAX_FILES)
            .map(|n| file(&alloc::format!("f{n}"), 1))
            .collect(),
    ));
    for case in &cases {
        assert_eq!(Plan::new(case, false).unwrap(), None, "{case:?}");
    }
    // The size of a font file is fine for font files, not for packs.
    let mut f = ok();
    f[0].size = MAX_PACK + 1;
    assert!(Plan::new(&font(f.clone()), true).unwrap().is_some());
    assert!(Plan::new(&font(f), false).unwrap().is_none());
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    let font = font(vec![
        file("plugin.ini", 100),
        file("fonts/regular.ppfp", 2000),
    ]);
    sweep(|| Plan::new(&font, false).map(|p| p.map(|p| p.steps.len())));
}
