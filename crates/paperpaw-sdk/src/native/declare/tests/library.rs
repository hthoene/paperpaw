use super::*;
use alloc::vec;
#[test]
fn book_field_and_badge_declarations_preserve_names_flags_and_token() {
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    d.book_field(
        BookFieldDecl::new("rating", "rating.title")
            .unwrap()
            .in_catalog(),
    )
    .unwrap();
    d.book_field(BookFieldDecl::new("loan", "loan.title").unwrap())
        .unwrap();
    d.book_badge("rating", 41).unwrap();
    d.submit().unwrap();
    drop(d);
    let batch = records_decode(&batches[0]).unwrap();
    let mut rows = batch.iter();
    assert!(
        matches!(rows.next(), Some(Record::BookFieldSpec(row)) if (row.name,row.title,row.flags)==("rating","rating.title",1))
    );
    assert!(
        matches!(rows.next(), Some(Record::BookFieldSpec(row)) if (row.name,row.flags)==("loan",0))
    );
    assert!(
        matches!(rows.next(), Some(Record::BookBadgeSpec(row)) if (row.name,row.token,row.flags)==("rating",41,0))
    );
    assert!(rows.next().is_none());
}
#[test]
fn book_declarations_refuse_bad_names_caps_and_latch_badge_refusals() {
    for name in ["", ".rating", "a..b", "Upper", "a\nname"] {
        assert!(BookFieldDecl::new(name, "Title").is_err());
    }
    assert!(BookFieldDecl::new("rating", "").is_err());
    assert!(BookFieldDecl::new(&"a".repeat(97), "Title").is_err());
    assert!(BookFieldDecl::new("rating", &"é".repeat(49)).is_err());
    assert!(BookFieldDecl::new(&"a".repeat(96), &"é".repeat(48)).is_ok());
    let mut batches = Vec::new();
    let mut d = collecting(&mut batches);
    assert!(d.book_badge("bad..name", 0).is_err());
    assert!(d.book_badge("valid", 1).is_err());
    assert!(d.submit().is_err());
    drop(d);
    assert_eq!(batches, vec![vec![]]);
}
