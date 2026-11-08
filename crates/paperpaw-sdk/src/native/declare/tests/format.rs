use super::*;
use alloc::vec;
#[test]
fn a_format_declares_its_name_extensions_flags_priority_and_token() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.format(
        FormatDecl::new("epub", "epub.title", "epub\nfb2.zip", 3)
            .priority(-2)
            .prepared()
            .sniffs(),
    )
    .unwrap();
    d.format(
        FormatDecl::new("images", "images.title", "cbz", 4)
            .folder()
            .whole_pages()
            .siblings(),
    )
    .unwrap();
    d.format(FormatDecl::new("txt", "txt.title", "txt", 5))
        .unwrap();
    d.submit().unwrap();
    drop(d);
    let rows: Vec<_> = records_decode(&batches[0]).unwrap().iter().collect();
    assert!(matches!(rows[0], Record::FormatSpec(s)
        if (s.name, s.title, s.extensions, s.flags, s.priority, s.token)
            == ("epub", "epub.title", "epub\nfb2.zip", 0b11, -2, 3)));
    assert!(matches!(rows[1], Record::FormatSpec(s) if s.flags == 0b11100 && s.token == 4));
    assert!(matches!(rows[2], Record::FormatSpec(s) if (s.flags, s.priority) == (0, 0)));
    assert_eq!(rows.len(), 3);
}
#[test]
fn a_format_is_refused_for_what_the_host_would_refuse() {
    let refuse = |decl: FormatDecl<'_>| {
        let mut batches = Vec::new();
        let mut d = collecting(&mut batches);
        let refused = d.format(decl).is_err();
        if !refused {
            d.submit().unwrap();
            drop(d);
            assert_eq!(batches.len(), 1);
        } else {
            drop(d);
        }
        refused
    };
    let bad_names = ["", "Epub", "a b", ".a", "a..b"];
    for name in bad_names {
        assert!(refuse(FormatDecl::new(name, "t", "x", 1)), "{name:?}");
    }
    assert!(refuse(FormatDecl::new("a", "", "x", 1)));
    assert!(refuse(FormatDecl::new("a", &"é".repeat(49), "x", 1)));
    assert!(!refuse(FormatDecl::new("a", &"é".repeat(48), "x", 1)));
    // Extensions: none, empty items, a dot in front, uppercase, control
    // characters, repeats, too long, too many.
    let long = "x".repeat(32);
    let many = (0..17)
        .map(|n| alloc::format!("e{n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let fits = (0..16)
        .map(|n| alloc::format!("e{n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let bad = vec![
        "", "\nx", "x\n", "x\n\ny", ".epub", "EPUB", "ep\tub", "a\nb\na", &long, &many,
    ];
    for extensions in bad {
        assert!(
            refuse(FormatDecl::new("a", "t", extensions, 1)),
            "{extensions:?}"
        );
    }
    assert!(!refuse(FormatDecl::new("a", "t", &fits, 1)));
    assert!(!refuse(FormatDecl::new("a", "t", &"x".repeat(31), 1)));
    assert!(!refuse(FormatDecl::new("a", "t", "fb2.zip\nzip", 1)));
    // A refusal latches the builder, as for every declaration.
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    assert!(d.format(FormatDecl::new("a", "", "x", 1)).is_err());
    assert!(d.format(FormatDecl::new("a", "t", "x", 1)).is_err());
}
