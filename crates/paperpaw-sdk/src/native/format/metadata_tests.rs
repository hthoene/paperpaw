use super::{Error, FormatCall, schema};
use crate::document::Metadata;

fn call<'a>(op: u32, kind: u32, index: u32, offset: u32, input: &'a mut [u8]) -> FormatCall<'a> {
    let n = schema::FmtCall::new(op, 7, 0, offset, index, 0, 0, kind, 0, 0, 22, "", &[])
        .encode(input)
        .unwrap();
    FormatCall::decode(&input[..n]).unwrap()
}
#[test]
fn metadata_previews_are_borrowed_and_utf8_cuts_are_advertised() {
    let metadata = Metadata::default()
        .title("界".repeat(200))
        .series("é".repeat(300))
        .authors(alloc::vec!["Author".into()])
        .subjects(alloc::vec!["Subject".into()]);
    let mut input = [0; 1024];
    let mut out = [0; 2048];
    let c = call(5, 0, 0, 0, &mut input);
    let n = c.reply_metadata(&metadata, 1, &mut out).unwrap();
    let row = schema::FmtMeta::decode(&out[..n]).unwrap();
    assert_eq!(
        (row.flags, row.title.len(), row.series.len()),
        (3, 510, 512)
    );
    assert_eq!((row.sections, row.authors, row.subjects), (1, 1, 1));
    assert_eq!(row.title, &metadata.title.as_ref().unwrap()[..510]);
    let c = call(6, 0, 0, 0, &mut input);
    let n = c.reply_metadata_text(&metadata, &mut out).unwrap();
    let row = schema::FmtText::decode(&out[..n]).unwrap();
    assert_eq!(row.text, metadata.title.as_ref().unwrap());
    assert_eq!(row.total, 600);
}
#[test]
fn metadata_text_pages_preserve_whole_values_and_refuse_bad_offsets() {
    let metadata = Metadata::default()
        .title("界".repeat(1000))
        .authors(alloc::vec!["Writer".into()]);
    let mut input = [0; 1024];
    let mut out = [0; 2048];
    let mut offset = 0;
    let mut joined = alloc::string::String::new();
    while offset < 3000 {
        let c = call(6, 0, 0, offset, &mut input);
        let n = c.reply_metadata_text(&metadata, &mut out).unwrap();
        let row = schema::FmtText::decode(&out[..n]).unwrap();
        assert_eq!((row.kind, row.index, row.total), (0, 0, 3000));
        assert!(row.text.len() <= 1024);
        joined.push_str(row.text);
        offset += u32::try_from(row.text.len()).unwrap();
    }
    assert_eq!(&joined, metadata.title.as_ref().unwrap());
    for offset in [1, 3001, u32::MAX] {
        let c = call(6, 0, 0, offset, &mut input);
        out.fill(0xa5);
        assert!(matches!(
            c.reply_metadata_text(&metadata, &mut out),
            Err(Error::Invalid)
        ));
        assert_eq!(out, [0xa5; 2048]);
    }
    let c = call(6, 1, 1, 0, &mut input);
    assert!(matches!(
        c.reply_metadata_text(&metadata, &mut out),
        Err(Error::NotFound)
    ));
    let c = call(6, 1, 0, 0, &mut input);
    let n = c.reply_metadata_text(&metadata, &mut out).unwrap();
    assert_eq!(schema::FmtText::decode(&out[..n]).unwrap().text, "Writer");
}
#[test]
fn metadata_caps_modes_and_output_fail_before_writing() {
    let mut input = [0; 1024];
    let mut out = [0xa5; 2048];
    let c = call(5, 0, 0, 0, &mut input);
    let metadata = Metadata::default().writing_mode(paperpaw_lang::WritingMode::SidewaysRl);
    assert!(matches!(
        c.reply_metadata(&metadata, 1, &mut out),
        Err(Error::Unsupported)
    ));
    assert_eq!(out, [0xa5; 2048]);
    assert!(matches!(
        c.reply_metadata(&Metadata::default(), 1, &mut out[..1]),
        Err(Error::NoSpace)
    ));
    assert_eq!(out, [0xa5; 2048]);
    let c = call(6, 3, 0, 0, &mut input);
    let n = c
        .reply_metadata_text(&Metadata::default(), &mut out)
        .unwrap();
    let row = schema::FmtText::decode(&out[..n]).unwrap();
    assert_eq!((row.total, row.text), (0, ""));
}

#[test]
fn an_odd_language_or_series_index_is_left_out_so_the_book_still_opens() {
    let mut input = [0; 1024];
    let mut out = [0; 2048];
    let c = call(5, 0, 0, 0, &mut input);
    let metadata = Metadata::default()
        .language("x".repeat(36))
        .series_index("1".repeat(33));
    let n = c.reply_metadata(&metadata, 1, &mut out).unwrap();
    let row = schema::FmtMeta::decode(&out[..n]).unwrap();
    assert_eq!((row.language, row.series_index), ("", ""));
    // At the caps they are kept.
    let metadata = Metadata::default()
        .language("x".repeat(35))
        .series_index("1".repeat(32));
    let n = c.reply_metadata(&metadata, 1, &mut out).unwrap();
    let row = schema::FmtMeta::decode(&out[..n]).unwrap();
    assert_eq!((row.language.len(), row.series_index.len()), (35, 32));
}
