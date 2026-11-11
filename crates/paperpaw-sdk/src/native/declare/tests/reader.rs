use super::*;
use alloc::vec;
#[test]
fn a_reader_declares_its_name_screen_and_formats() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.reader(ReaderDecl::new("reader", "reader.screen"))
        .unwrap();
    d.reader(
        ReaderDecl::new("comics", "reader.comics").formats("format-images.cbz\nformat-xtc.xtc"),
    )
    .unwrap();
    d.submit().unwrap();
    drop(d);
    let rows: Vec<_> = records_decode(&batches[0]).unwrap().iter().collect();
    assert!(matches!(rows[0], Record::ReaderSpec(s)
        if (s.name, s.screen, s.formats, s.flags) == ("reader", "reader.screen", "", 0)));
    assert!(matches!(rows[1], Record::ReaderSpec(s)
        if s.formats == "format-images.cbz\nformat-xtc.xtc"));
    assert_eq!(rows.len(), 2);
}
#[test]
fn a_reader_is_refused_for_what_the_host_would_refuse() {
    let refuse = |decl: ReaderDecl<'_>| {
        let mut batches = Vec::new();
        let mut d = collecting(&mut batches);
        let refused = d.reader(decl).is_err();
        if !refused {
            d.submit().unwrap();
        }
        refused
    };
    for name in ["", "Reader", "a b", ".a", "a..b"] {
        assert!(refuse(ReaderDecl::new(name, "p.screen")), "{name:?}");
    }
    // The screen is a full id.
    for screen in ["", "screen", "p..s", "P.s"] {
        assert!(refuse(ReaderDecl::new("r", screen)), "{screen:?}");
    }
    let many = (0..9)
        .map(|n| alloc::format!("p.f{n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let fits = (0..8)
        .map(|n| alloc::format!("p.f{n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let bad = vec!["\n", "p.f\n", "f", "p.f\nf", "P.F", &many];
    for formats in bad {
        assert!(
            refuse(ReaderDecl::new("r", "p.s").formats(formats)),
            "{formats:?}"
        );
    }
    assert!(!refuse(ReaderDecl::new("r", "p.s").formats(&fits)));
    assert!(!refuse(ReaderDecl::new("r", "p.s").formats("p.f")));
}
