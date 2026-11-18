use super::*;
use paperpaw_sdk::MemoryStorage;

fn shared(storage: MemoryStorage) -> Rc<dyn Storage> {
    Rc::new(storage)
}

/// Every data plugin in `dir` that loads, the others reported.
fn discover(storage: &Rc<dyn Storage>, dir: &str, warn: &mut dyn FnMut(String)) -> Vec<DataPlugin> {
    folders(storage.as_ref(), dir, warn)
        .iter()
        .filter_map(|path| load_folder(storage, path).map_err(&mut *warn).ok())
        .collect()
}

#[test]
fn plugins_switched_off_keep_only_what_lists_them() {
    let source = "[meta]\nlanguage = de\nname = Deutsch\n\n[messages]\nbig.plugin.name = Groß\nbig.plugin.description = Größer.\nreader.title = Lesen\n";
    assert_eq!(
        listing(source, "big.plugin."),
        "[meta]\nlanguage = de\nname = Deutsch\n\n[messages]\nbig.plugin.name = Groß\nbig.plugin.description = Größer.\n"
    );
}

#[test]
fn finds_folders_with_a_manifest() {
    let storage = MemoryStorage::new()
        .with(
            "plugins/large/plugin.ini",
            "[plugin]\nname = Large print\ndescription = Bigger text.\nversion = 1.2.0\n",
        )
        .with("plugins/large/settings.ini", "[core]\nui-size = 14\n")
        .with("plugins/large/styles.css", "p { line-height: 1.6 }")
        .with("plugins/large/i18n/de.ini", "[meta]\nlanguage = de\n")
        .with("plugins/large/i18n/FR.INI", "[meta]\nlanguage = fr\n")
        .with("plugins/broken/readme.txt", "")
        .with("plugins/bad-id/plugin.ini", "[plugin]\nid = a.b\n");
    let mut warnings = Vec::new();
    let storage = shared(storage);
    let plugins = discover(&storage, "plugins", &mut |w| warnings.push(w));
    assert_eq!(plugins.len(), 1);
    let large = &plugins[0];
    assert_eq!(large.manifest.id.as_str(), "large");
    assert_eq!(large.manifest.name, "Large print");
    assert_eq!(large.manifest.version, Version::new(1, 2, 0));
    assert_eq!(large.translations.borrow().len(), 2, "any case");
    assert!(
        large
            .later
            .borrow()
            .iter()
            .any(|p| p.ends_with("settings.ini"))
    );
    assert!(large.keymap.borrow().is_none());
    assert_eq!(
        large.styles.borrow().as_deref(),
        Some("p { line-height: 1.6 }")
    );
    assert_eq!(warnings.len(), 2, "{warnings:?}");
}

#[test]
fn a_pre_release_a_version_does_not_hold_is_refused_saying_so() {
    let storage = MemoryStorage::new()
        .with(
            "plugins/a/plugin.ini",
            "[plugin]
name = A
version = 1.0.0-beta.2
",
        )
        .with(
            "plugins/b/plugin.ini",
            "[plugin]
name = B
version = 1.0.0-gamma
",
        );
    let mut warnings = Vec::new();
    let storage = shared(storage);
    let plugins = discover(&storage, "plugins", &mut |w| warnings.push(w));
    assert_eq!(plugins.len(), 1, "{warnings:?}");
    assert_eq!(plugins[0].manifest.version.pre_name(), Some("beta"));
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("invalid version `1.0.0-gamma`")
            && warnings[0].contains("alpha, beta, dev, pre or rc"),
        "{warnings:?}"
    );
}

#[test]
fn a_packs_file_name_gives_its_family_and_face() {
    let of = |family: &str, key| (String::from(family), key);
    assert_eq!(
        pack_face("Literata-BoldItalic.ppfp"),
        of("Literata", "bold-italic")
    );
    assert_eq!(
        pack_face("Literata-Bold-Italic.ppfp"),
        of("Literata", "bold-italic")
    );
    assert_eq!(
        pack_face("Noto Serif Bold Italic.ppfp"),
        of("Noto Serif", "bold-italic")
    );
    assert_eq!(
        pack_face("noto_serif_italic.PPFP"),
        of("noto_serif", "italic")
    );
    assert_eq!(pack_face("Fredoka-Oblique.ppfp"), of("Fredoka", "italic"));
    assert_eq!(
        pack_face("Fredoka-BoldOblique.ppfp"),
        of("Fredoka", "bold-italic")
    );
    assert_eq!(pack_face("Fredoka-Bold.ppfp"), of("Fredoka", "bold"));
    assert_eq!(pack_face("Fredoka-Regular.ppfp"), of("Fredoka", "regular"));
    // No style: the regular face; a style alone or run into the
    // name is no style.
    assert_eq!(pack_face("Bookerly.ppfp"), of("Bookerly", "regular"));
    assert_eq!(pack_face("Bold.ppfp"), of("Bold", "regular"));
    assert_eq!(pack_face("Seabold.ppfp"), of("Seabold", "regular"));
    assert_eq!(pack_face("Ä-Bold.ppfp"), of("Ä", "bold"));
    assert_eq!(pack_face("ÄİBold.ppfp"), of("ÄİBold", "regular"));
}

#[test]
fn packs_loose_in_a_font_folder_are_faces_of_their_families() {
    let storage = shared(
        MemoryStorage::new()
            .with("fonts/Literata-Regular.ppfp", "x")
            .with("fonts/Literata-Bold.ppfp", "x")
            .with("fonts/greek/Literata-Italic.ppfp", "x")
            .with("fonts/Bookerly.ppfp", "x")
            .with("other/Bookerly.ppfp", "x")
            .with("fonts/notes.txt", "")
            .with("fonts/._Literata-Bold.ppfp", "x"),
    );
    let dirs = [String::from("fonts"), String::from("other")];
    let folder = font_folder(storage.as_ref(), &dirs).unwrap();
    assert!(folder.fonts.borrow().is_empty());
    let mut r = Staging::new(Id::new_static("font-folder"));
    folder.register(&mut r).unwrap();
    let faces: Vec<(String, String, bool, bool)> = r
        .font_faces
        .iter()
        .map(|face| {
            (
                face.id.to_string(),
                String::from(face.family.as_ref()),
                face.bold,
                face.italic,
            )
        })
        .collect();
    let face = |id: &str, family: &str, bold, italic| {
        (String::from(id), String::from(family), bold, italic)
    };
    assert_eq!(
        faces,
        [
            face("font-folder.bookerly-regular", "Bookerly", false, false),
            face("font-folder.literata-bold", "Literata", true, false),
            face("font-folder.literata-regular", "Literata", false, false),
            face("font-folder.literata-italic", "Literata", false, true),
            // The same name in another folder: kept apart.
            face("font-folder.bookerly-regular-2", "Bookerly", false, false),
        ]
    );
    // A plugin's own packs are named in its plugin.ini: not fonts.
    let plugin = shared(
        MemoryStorage::new()
            .with("plugins/greek/plugin.ini", "[plugin]\n")
            .with("plugins/greek/fonts/literata.ppfp", "x"),
    );
    let found = discover(&plugin, "plugins", &mut |_| {});
    assert!(found[0].fonts.borrow().is_empty());
}

#[test]
fn finds_fonts_in_plugins_and_font_folders() {
    let storage = MemoryStorage::new()
        .with(
            "plugins/serif/plugin.ini",
            "[plugin]
",
        )
        .with("plugins/serif/fonts/Book Antiqua-Bold.TTF", "x")
        .with("plugins/serif/fonts/readme.txt", "")
        .with("fonts/a.otf", "x")
        .with("more-fonts/b.ttc", "x")
        .with("more-fonts/family/c.ttf", "x");
    let storage = shared(storage);
    let plugins = discover(&storage, "plugins", &mut |_| {});
    assert_eq!(
        *plugins[0].fonts.borrow(),
        ["plugins/serif/fonts/Book Antiqua-Bold.TTF"]
    );
    let dirs = [String::from("fonts"), String::from("more-fonts")];
    // Scalable fonts only where they are drawn.
    let folder = font_folder(storage.as_ref(), &dirs);
    match cfg!(feature = "outline") {
        true => assert_eq!(
            *folder.unwrap().fonts.borrow(),
            ["fonts/a.otf", "more-fonts/b.ttc", "more-fonts/family/c.ttf"]
        ),
        false => assert!(folder.is_none()),
    }
    assert!(font_folder(storage.as_ref(), &[String::from("nothing")]).is_none());
    assert_eq!(slug("Book Antiqua-Bold.TTF"), "book-antiqua-bold");
    // Packs that extend other fonts, named in plugin.ini.
    let source = "[plugin]
id = cyrillic
[extends]
literata.ppfp = fonts-default.literata
";
    assert_eq!(
        extensions(source, "plugins/cyrillic"),
        [(
            String::from("plugins/cyrillic/fonts/literata.ppfp"),
            String::from("fonts-default.literata")
        )]
    );
    // One pack for several fonts.
    let source = "[extends]\nbengali.ppfp = fonts-default.literata, fonts-default.source-sans\n";
    let pack = String::from("plugins/b/fonts/bengali.ppfp");
    assert_eq!(
        extensions(source, "plugins/b"),
        [
            (pack.clone(), String::from("fonts-default.literata")),
            (pack, String::from("fonts-default.source-sans"))
        ]
    );
    assert_eq!(slug("….ttf"), "font");
}

#[test]
fn families_name_their_faces() {
    let storage = MemoryStorage::new()
        .with(
            "plugins/font-round/plugin.ini",
            "[plugin]
name = Round
[family]
name = Round
regular = Round.ttf
bold = round-bold.ppfp
oblique = nothing.ppfp
italic = ../escape.ppfp
",
        )
        .with("plugins/font-round/fonts/Round.ttf", "x")
        .with("plugins/font-round/fonts/Extra.ttf", "x");
    let plugin = load_folder(&shared(storage), "plugins/font-round/").unwrap();
    assert_eq!(plugin.folder(), "plugins/font-round");
    assert_eq!(plugin.family.as_deref(), Some("Round"));
    assert_eq!(
        *plugin.faces.borrow(),
        [
            (
                "regular",
                String::from("plugins/font-round/fonts/Round.ttf")
            ),
            (
                "bold",
                String::from("plugins/font-round/fonts/round-bold.ppfp")
            ),
        ]
    );
    // A face is no font of its own as well.
    assert_eq!(
        *plugin.fonts.borrow(),
        ["plugins/font-round/fonts/Extra.ttf"]
    );
    // A face of a family without its regular face: e.g. a bold
    // italic face the firmware leaves out.
    assert_eq!(
        family(
            "[family]
bold-italic = a.ppfp
name = Literata
",
            "p"
        ),
        (
            Some(String::from("Literata")),
            alloc::vec![("bold-italic", String::from("p/fonts/a.ppfp"))]
        )
    );
    // No family without a name, nor without a face.
    assert_eq!(
        family(
            "[family]
name = A
",
            "p"
        ),
        (None, Vec::new())
    );
    assert_eq!(
        family(
            "[family]
regular = a.ppfp
",
            "p"
        ),
        (None, Vec::new())
    );
}

#[test]
fn presets_are_read_a_section_each() {
    let source = "# Layouts
[nook]
kind = home.layout
name = Reading nook
                  description = The book, large.
home.page-1 = home.continue 0/0 4x3
                  home.rows = 4
[plain]
home.status-bar = off
";
    let presets = presets(&Id::new_static("cosy"), source).unwrap();
    assert_eq!(presets.len(), 2);
    let nook = &presets[0];
    assert_eq!(nook.id.as_str(), "cosy.nook");
    assert_eq!(nook.kind.as_str(), "home.layout");
    assert_eq!(nook.title, "Reading nook");
    assert_eq!(nook.value("home.page-1"), Some("home.continue 0/0 4x3"));
    assert_eq!(nook.value("home.rows"), Some("4"));
    // Without a kind, the plugin's own.
    assert_eq!(presets[1].kind.as_str(), "cosy");
    assert!(
        super::presets(
            &Id::new_static("cosy"),
            "[a]
kind = Not An Id
"
        )
        .is_err()
    );
}

#[test]
fn plugins_made_for_later_formats_are_left_out() {
    let ini = |formats: &str| format!("[plugin]\nname = Large print\n{formats}");
    assert!(parse_manifest(&ini(""), "large").is_ok());
    assert!(parse_manifest(&ini("formats = 1\n"), "large").is_ok());
    assert!(parse_manifest(&ini("formats = 2\n"), "large").is_ok());
    assert_eq!(parse_manifest(&ini(""), "large").unwrap().1, 1);
    assert!(parse_manifest(&ini("formats = 3\n"), "large").is_ok());
    assert!(parse_manifest(&ini("formats = 4\n"), "large").is_ok());
    assert!(parse_manifest(&ini("formats = 5\n"), "large").is_ok());
    let later = parse_manifest(&ini("formats = 6\n"), "large").unwrap_err();
    assert!(later.contains("later PaperPaw"), "{later}");
    assert!(parse_manifest(&ini("formats = new\n"), "large").is_err());
}

#[test]
fn the_host_id_and_overlong_ids_are_refused() {
    let ok = |id: &str| parse_manifest(&format!("[plugin]\nid = {id}\n"), "x").is_ok();
    let long = "a".repeat(64);
    assert!(ok("core-tools") && ok(&long) && !ok(&format!("{long}a")));
    assert!(!ok("core") && !ok("core.evil") && parse_manifest("", "core").is_err());
}

/// `requires` names plugins as said, in formats 4 and later; else the
/// plugin is not loaded, with why.
#[test]
fn requirements_are_checked_with_the_manifest() {
    let ini = |rest: &str| format!("[plugin]\nname = Cosy\n{rest}");
    assert!(parse_manifest(&ini("formats = 4\nrequires = stats 1.0.0, news\n"), "cosy").is_ok());
    let old = parse_manifest(&ini("formats = 3\nrequires = stats\n"), "cosy").unwrap_err();
    assert!(old.contains("formats 4"), "{old}");
    let own = parse_manifest(&ini("formats = 4\nrequires = cosy\n"), "cosy").unwrap_err();
    assert!(own.contains("itself"), "{own}");
    assert!(parse_manifest(&ini("formats = 4\nrequires = Not Ok\n"), "cosy").is_err());
}

/// Fonts bound to roles in `[fonts]` of `plugin.ini`.
#[test]
fn fonts_are_bound_to_roles() {
    assert_eq!(
        roles(
            "[plugin]
id = x
[fonts]
core.font-ui = x.semi
"
        ),
        [(String::from("core.font-ui"), String::from("x.semi"))]
    );
}

/// Files of formats 3 are read only for a plugin made for them; they
/// are held within a budget, and lines for points within a limit.
#[test]
fn files_of_formats_3_are_read_within_limits() {
    let older = MemoryStorage::new()
        .with(
            "plugins/old/plugin.ini",
            "[plugin]\nformats = 2\n[fonts]\ncore.font-ui = old.x\n",
        )
        .with("plugins/old/menus.ini", "[old.m]\ntitle = M\n");
    let plugin = load_folder(&shared(older), "plugins/old").unwrap();
    assert!(plugin.later.borrow().is_empty());
    assert!(plugin.roles.borrow().is_empty());
    assert_eq!(plugin.take_problems().len(), 2);
    let lines: String = (0..MAX_POINT_LINES + 5)
        .map(|n| format!("line {n}\n"))
        .collect();
    let big = alloc::vec![b'#'; MAX_FORMAT3_DATA + 1];
    let newer = MemoryStorage::new()
        .with("plugins/new/plugin.ini", "[plugin]\nformats = 3\n")
        .with("plugins/new/points/core.about.txt", lines.as_str())
        .with("plugins/new/actions.ini", big);
    let plugin = load_folder(&shared(newer), "plugins/new").unwrap();
    let later = plugin.later.borrow().clone();
    assert_eq!(later, ["plugins/new/points/core.about.txt"], "too large");
    assert_eq!(plugin.take_problems().len(), 1);
    let mut r = Staging::new(Id::new_static("new"));
    plugin.register(&mut r).unwrap();
    let lines = r.contributions.len();
    assert_eq!(lines, MAX_POINT_LINES);
    // All files of a plugin together: within one budget.
    let file = alloc::vec![b'#'; MAX_FILE - 1];
    let many = MemoryStorage::new()
        .with("plugins/many/plugin.ini", "[plugin]\n")
        .with("plugins/many/keymap.ini", file.clone())
        .with("plugins/many/settings.ini", file.clone())
        .with("plugins/many/presets.ini", file);
    let plugin = load_folder(&shared(many), "plugins/many").unwrap();
    assert!(plugin.keymap.borrow().is_some());
    assert_eq!(plugin.later.borrow().len(), 0);
    assert_eq!(plugin.take_problems().len(), 2);
}
