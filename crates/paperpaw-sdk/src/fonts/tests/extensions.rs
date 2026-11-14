//! Packs that extend a font are open while that font is.

use alloc::rc::Rc;
use alloc::string::String;

use super::pack;
use crate::fonts::{Font, FontFace, FontSpec, Fonts, Priority, roles};
use crate::{Id, MemoryStorage, Storage};

/// A script's letters for a face of the card (Greek for Literata, which
/// the firmware leaves to a plugin) are open while that face is, and
/// those for a built-in font always. A pack for a face that sets no text
/// now, or for a font nobody added, is not opened: its index would take
/// memory for nothing, and its file is not missed.
#[test]
fn a_pack_extending_a_face_is_open_only_while_the_face_is() {
    let storage: Rc<dyn Storage> = Rc::new(
        MemoryStorage::new()
            .with("plugins/serif/fonts/serif.ppfp", pack("a"))
            .with("plugins/el/fonts/el-serif.ppfp", pack("\u{3b1}"))
            .with("plugins/el/fonts/el-sans.ppfp", pack("\u{3b1}"))
            .with("plugins/el/fonts/el-ghost.ppfp", pack("\u{3b1}")),
    );
    let mut fonts = Fonts::new(4096);
    let sans = Font::from_data(pack("a"), &[]).unwrap();
    fonts
        .add(FontSpec::new(Id::new_static("a.sans"), sans).family("Sans", false, false))
        .unwrap();
    fonts.propose(roles::READING, Id::new_static("a.sans"), Priority::DEFAULT);
    fonts
        .add_face(FontFace::new(
            Id::new_static("serif.regular"),
            "plugins/serif/fonts/serif.ppfp",
            "Serif",
            false,
            false,
        ))
        .unwrap();
    for (id, file, extends) in [
        ("el.serif", "el-serif", "serif.regular"),
        ("el.sans", "el-sans", "a.sans"),
        ("el.ghost", "el-ghost", "nobody.font"),
        // Its file is not on the card: missed only once its face is open.
        ("el.late", "el-late", "serif.regular"),
    ] {
        fonts
            .add_extension(
                Id::new_static(id),
                alloc::format!("plugins/el/fonts/{file}.ppfp"),
                Id::new_static(extends),
            )
            .unwrap();
    }
    let open = |fonts: &Fonts| {
        ["el.serif", "el.sans", "el.ghost", "el.late"].map(|id| fonts.get(id).is_some())
    };

    assert!(fonts.load(&storage).is_empty());
    assert_eq!(open(&fonts), [false, true, false, false]);
    let reading = fonts.role(roles::READING.as_str()).unwrap();
    assert!(reading.has_char('\u{3b1}'), "the built-in font's Greek");

    fonts.set_family(Some("Serif"));
    let failed = fonts.load(&storage);
    assert_eq!(failed, [String::from("plugins/el/fonts/el-late.ppfp")]);
    assert_eq!(open(&fonts), [true, true, false, false]);
    let reading = fonts.role(roles::READING.as_str()).unwrap();
    assert_eq!(reading.id().as_str(), "serif.regular");
    assert!(reading.has_char('\u{3b1}'), "the face's Greek");

    fonts.set_family(None);
    assert!(fonts.load(&storage).is_empty());
    assert_eq!(open(&fonts), [false, true, false, false]);
}
