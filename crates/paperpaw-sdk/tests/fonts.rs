//! Fonts on the storage: damaged, empty, missing or huge files are
//! reported by `Fonts::load` and never loaded, any bytes are read without
//! panic, a family's face nearest each style's weight is chosen, and the
//! roles fall back to fonts that are there.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_sdk::builtin::fonts as roles;
use paperpaw_sdk::text::sfnt::FontInfo;
use paperpaw_sdk::{
    Font, FontFace, FontSpec, Fonts, Id, LineMetrics, MemoryStorage, Priority, RegisterError,
    Storage,
};
use paperpaw_text::{Glyph, PackBuilder};
use proptest::prelude::*;

/// A font pack holding `chars`, each `advance` wide.
fn pack(chars: &str, advance: f32) -> Vec<u8> {
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    let mut builder = PackBuilder::new();
    let glyphs = chars.chars().map(|c| (c, Glyph::empty(advance))).collect();
    builder.add_strike(10.0, metrics, glyphs);
    builder.build()
}

/// Fonts with a built-in one proposed for reading.
fn with_builtin() -> Fonts {
    let mut fonts = Fonts::new(4096);
    let builtin = Font::from_data(pack("ab", 5.0), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.builtin"), builtin).family("Builtin", false, false))
        .unwrap();
    fonts.propose(
        roles::READING,
        Id::new_static("a.builtin"),
        Priority::DEFAULT,
    );
    fonts
}

fn info(family: &str, bold: bool, italic: bool, weight: u16) -> FontInfo {
    FontInfo::new(family, bold, italic, weight)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)))]

    /// Whatever bytes a font file, a face's pack or an extension holds,
    /// loading does not panic; what cannot be read is reported and the
    /// reading font stays one that is there.
    #[test]
    fn any_bytes_as_font_files_are_loaded_without_panic(
        file in proptest::collection::vec(any::<u8>(), 0..200),
        cut in 0usize..400,
    ) {
        let good = pack("abc", 6.0);
        let mut damaged = good.clone();
        damaged.truncate(cut.min(good.len()));
        let storage: Rc<dyn Storage> = Rc::new(
            MemoryStorage::new()
                .with("f/any.ttf", file.clone())
                .with("f/any.ppfp", file.clone())
                .with("f/cut.ppfp", damaged)
                .with("f/ext.ppfp", file),
        );
        let mut fonts = with_builtin();
        fonts.add_file(Id::new_static("x.any"), "f/any.ttf".into(), info("Any", false, false, 400)).unwrap();
        fonts.add_face(FontFace::new(Id::new_static("x.any-bold"), "f/any.ppfp", "Any", true, false)).unwrap();
        fonts.add_face(FontFace::new(Id::new_static("x.cut"), "f/cut.ppfp", "Any", false, true)).unwrap();
        fonts.add_extension(Id::new_static("x.ext"), "f/ext.ppfp".into(), Id::new_static("a.builtin")).unwrap();
        fonts.set_family(Some("Any"));
        let failed = fonts.load(&storage);
        for (id, path) in [("x.any", "f/any.ttf"), ("x.any-bold", "f/any.ppfp"), ("x.cut", "f/cut.ppfp")] {
            prop_assert_eq!(fonts.get(id).is_none(), failed.iter().any(|f| f == path), "{}", id);
        }
        // A reading font is there, whatever failed.
        let reading = fonts.role(roles::READING.as_str());
        prop_assert!(reading.is_some());
        let _ = reading.unwrap().measure(10.0, "abc ж");
    }
}

#[test]
fn missing_empty_and_damaged_files_are_reported() {
    let storage: Rc<dyn Storage> = Rc::new(
        MemoryStorage::new()
            .with("f/empty.ttf", Vec::new())
            .with("f/text.ttf", "not a font")
            .with("f/bold.ppfp", pack("ab", 5.0)),
    );
    let mut fonts = with_builtin();
    fonts
        .add_file(
            Id::new_static("x.regular"),
            "f/missing.ttf".into(),
            info("X", false, false, 400),
        )
        .unwrap();
    fonts
        .add_file(
            Id::new_static("x.italic"),
            "f/empty.ttf".into(),
            info("X", false, true, 400),
        )
        .unwrap();
    fonts
        .add_file(
            Id::new_static("x.bold-italic"),
            "f/text.ttf".into(),
            info("X", true, true, 700),
        )
        .unwrap();
    fonts
        .add_face(FontFace::new(
            Id::new_static("x.bold"),
            "f/bold.ppfp",
            "X",
            true,
            false,
        ))
        .unwrap();
    fonts.set_family(Some("X"));
    let mut failed = fonts.load(&storage);
    failed.sort();
    assert_eq!(failed, ["f/empty.ttf", "f/missing.ttf", "f/text.ttf"]);
    // The family's regular face failed: the plugins' proposal reads.
    assert_eq!(
        fonts.role(roles::READING.as_str()).unwrap().id().as_str(),
        "a.builtin"
    );
    assert_eq!(
        fonts
            .bound(roles::READING_BOLD.as_str())
            .unwrap()
            .id()
            .as_str(),
        "x.bold"
    );
    // The family's own faces that failed are not bound to another's.
    assert!(fonts.bound(roles::READING_ITALIC.as_str()).is_none());
    // Loading again tries again, and reports again.
    assert_eq!(fonts.load(&storage).len(), 3);
}

#[test]
fn the_face_nearest_each_weight_is_loaded() {
    let storage: Rc<dyn Storage> = Rc::new(
        MemoryStorage::new()
            .with("f/light.bin", pack("a", 1.0))
            .with("f/medium.bin", pack("a", 2.0))
            .with("f/semibold.bin", pack("a", 3.0))
            .with("f/black.bin", pack("a", 4.0)),
    );
    let mut fonts = with_builtin();
    for (id, path, bold, weight) in [
        ("w.light", "f/light.bin", false, 300),
        ("w.medium", "f/medium.bin", false, 450),
        ("w.semibold", "f/semibold.bin", true, 600),
        ("w.black", "f/black.bin", true, 900),
    ] {
        fonts
            .add_file(
                Id::new_static(id),
                path.into(),
                info("W", bold, false, weight),
            )
            .unwrap();
    }
    fonts.set_family(Some("W"));
    assert!(fonts.load(&storage).is_empty());
    let width = |role: &paperpaw_sdk::Id| fonts.bound(role.as_str()).unwrap().measure(10.0, "a");
    // 450 is nearer 400 than 300; 600 nearer 700 than 900.
    assert_eq!(width(&roles::READING), 2.0);
    assert_eq!(width(&roles::READING_BOLD), 3.0);
    // Only the faces wanted are loaded.
    assert!(fonts.get("w.light").is_none() && fonts.get("w.black").is_none());
    assert_eq!(fonts.families(), ["Builtin", "W"]);
    assert_eq!(fonts.family(), Some("W"));
}

#[test]
fn files_larger_than_fonts_may_be_are_not_read() {
    // A file far beyond any font is refused before it is read.
    struct Huge;
    impl Storage for Huge {
        fn read_at(
            &self,
            _: &str,
            _: u64,
            buf: &mut [u8],
        ) -> Result<usize, paperpaw_sdk::StorageError> {
            Ok(buf.len())
        }
        fn size(&self, _: &str) -> Result<u64, paperpaw_sdk::StorageError> {
            Ok(u64::MAX)
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
            Err(paperpaw_sdk::StorageError::Unsupported)
        }
        fn append(&self, _: &str, _: &[u8]) -> Result<(), paperpaw_sdk::StorageError> {
            Err(paperpaw_sdk::StorageError::Unsupported)
        }
        fn list(
            &self,
            _: &str,
            _: &mut dyn FnMut(paperpaw_sdk::DirEntry<'_>),
        ) -> Result<(), paperpaw_sdk::StorageError> {
            Err(paperpaw_sdk::StorageError::Unsupported)
        }
        fn remove(&self, _: &str) -> Result<(), paperpaw_sdk::StorageError> {
            Err(paperpaw_sdk::StorageError::Unsupported)
        }
    }
    let storage: Rc<dyn Storage> = Rc::new(Huge);
    let mut fonts = with_builtin();
    fonts
        .add_file(
            Id::new_static("h.huge"),
            "huge.ttf".into(),
            info("H", false, false, 400),
        )
        .unwrap();
    fonts.set_family(Some("H"));
    assert_eq!(fonts.load(&storage), ["huge.ttf"]);
}

#[test]
fn fonts_are_registered_once_and_bound_as_chosen() {
    let mut fonts = with_builtin();
    let other = Font::from_data(pack("xyz", 7.0), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("b.other"), other))
        .unwrap();
    for duplicate in [
        fonts.add_file(
            Id::new_static("a.builtin"),
            "x".into(),
            info("X", false, false, 400),
        ),
        fonts.add_extension(
            Id::new_static("b.other"),
            "x".into(),
            Id::new_static("a.builtin"),
        ),
        fonts.add_face(FontFace::new(
            Id::new_static("a.builtin"),
            "x",
            "X",
            false,
            false,
        )),
    ] {
        assert!(matches!(duplicate, Err(RegisterError::Duplicate { .. })));
    }
    assert!(fonts.contains("b.other") && !fonts.contains("b.none"));
    assert_eq!(fonts.ids().count(), 2);
    // The user's choice wins over proposals; one not there does not.
    assert!(!fonts.user_bound(roles::READING.as_str()));
    fonts.bind_user(roles::READING, Id::new_static("b.none"));
    assert_eq!(
        fonts.role(roles::READING.as_str()).unwrap().id().as_str(),
        "a.builtin"
    );
    fonts.bind_user(roles::READING, Id::new_static("b.other"));
    assert!(fonts.user_bound(roles::READING.as_str()));
    assert_eq!(
        fonts.role(roles::READING.as_str()).unwrap().id().as_str(),
        "b.other"
    );
    // A higher proposal wins among proposals; a role nobody proposed
    // takes the first font there.
    fonts.propose(roles::UI, Id::new_static("a.builtin"), Priority::FALLBACK);
    fonts.propose(roles::UI, Id::new_static("b.other"), Priority::PREFERRED);
    assert_eq!(
        fonts.role(roles::UI.as_str()).unwrap().id().as_str(),
        "b.other"
    );
    assert_eq!(
        fonts.role("x.unknown-role").unwrap().id().as_str(),
        "a.builtin"
    );
    assert!(fonts.bound("x.unknown-role").is_none());
    // Removing a plugin's fonts takes its proposals too.
    assert_eq!(fonts.remove_plugin("b"), 1);
    assert_eq!(
        fonts.role(roles::UI.as_str()).unwrap().id().as_str(),
        "a.builtin"
    );
    assert_eq!(fonts.remove_plugin("b"), 0);
    fonts.shrink_to_fit();
    assert!(format!("{fonts:?}").contains("a.builtin"));
}

#[test]
fn status_lines_take_their_own_family_or_the_books() {
    use paperpaw_sdk::builtin::settings::{STATUS_FONT_BOOK, STATUS_FONT_UI};
    let storage: Rc<dyn Storage> = Rc::new(MemoryStorage::new().with("f/s.ppfp", pack("s", 9.0)));
    let mut fonts = with_builtin();
    let ui = Font::from_data(pack("u", 3.0), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.ui"), ui))
        .unwrap();
    fonts.propose(roles::UI, Id::new_static("a.ui"), Priority::DEFAULT);
    fonts
        .add_face(FontFace::new(
            Id::new_static("s.small"),
            "f/s.ppfp",
            "Small",
            false,
            false,
        ))
        .unwrap();
    let status = |fonts: &Fonts| {
        fonts
            .role(roles::STATUS.as_str())
            .unwrap()
            .id()
            .as_str()
            .to_string()
    };
    assert_eq!(status(&fonts), "a.ui");
    fonts.set_status(STATUS_FONT_BOOK);
    assert_eq!(status(&fonts), "a.builtin");
    fonts.set_status("Small");
    // Its family is loaded for it, though not chosen for reading.
    assert!(fonts.load(&storage).is_empty());
    assert_eq!(status(&fonts), "s.small");
    fonts.set_status(STATUS_FONT_UI);
    assert!(fonts.load(&storage).is_empty());
    assert_eq!(status(&fonts), "a.ui");
    assert!(fonts.get("s.small").is_none(), "freed");
    // A family that is not there: the menus' font.
    fonts.set_status("Nowhere");
    assert!(fonts.load(&storage).is_empty());
    assert_eq!(status(&fonts), "a.ui");
    assert_eq!(fonts.cache_bytes(), 0);
}
