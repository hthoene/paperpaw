use super::*;

#[test]
fn test_split_path() {
    assert_eq!(split_path("aaa/bbb/ccc"), ("aaa", Some("bbb/ccc")));
    assert_eq!(split_path("aaa/bbb"), ("aaa", Some("bbb")));
    assert_eq!(split_path("aaa"), ("aaa", None));
}

#[test]
fn test_generate_short_name() {
    assert_eq!(
        ShortNameGenerator::new("Foo").generate().ok(),
        Some(*b"FOO        ")
    );
    assert_eq!(
        ShortNameGenerator::new("Foo.b").generate().ok(),
        Some(*b"FOO     B  ")
    );
    assert_eq!(
        ShortNameGenerator::new("Foo.baR").generate().ok(),
        Some(*b"FOO     BAR")
    );
    assert_eq!(
        ShortNameGenerator::new("Foo+1.baR").generate().ok(),
        Some(*b"FOO_1~1 BAR")
    );
    assert_eq!(
        ShortNameGenerator::new("ver +1.2.text").generate().ok(),
        Some(*b"VER_12~1TEX")
    );
    assert_eq!(
        ShortNameGenerator::new(".bashrc.swp").generate().ok(),
        Some(*b"BASHRC~1SWP")
    );
}

#[test]
fn test_generate_short_name_multibyte() {
    assert_eq!(
        ShortNameGenerator::new("日本語.txt").generate().ok(),
        Some(*b"___~1   TXT")
    );
}

#[test]
fn test_generate_short_name_leading_dot() {
    assert_eq!(
        ShortNameGenerator::new(".foo").generate().ok(),
        Some(*b"FOO~1      ")
    );
}

#[test]
fn test_short_name_checksum_overflow() {
    ShortNameGenerator::checksum("\u{FF5A}\u{FF5A}\u{FF5A}\u{FF5A}");
}

#[test]
fn test_lfn_checksum_overflow() {
    lfn_checksum(&[
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ]);
}

#[test]
fn test_generate_short_name_collisions_long() {
    let mut buf: [u8; SFN_SIZE];
    let mut names = ShortNameGenerator::new("TextFile.Mine.txt");
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"TEXTFI~1TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"TEXTFI~2TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"TEXTFI~3TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"TEXTFI~4TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"TE527D~1TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"TE527D~2TXT");
    for i in 3..10 {
        names.add_existing(&buf);
        buf = names.generate().unwrap();
        assert_eq!(&buf, format!("TE527D~{}TXT", i).as_bytes());
    }
    names.add_existing(&buf);
    assert!(names.generate().is_err());
    names.next_iteration();
    for _i in 0..4 {
        buf = names.generate().unwrap();
        names.add_existing(&buf);
    }
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"TE527E~1TXT");
}

#[test]
fn test_generate_short_name_collisions_short() {
    let mut buf: [u8; SFN_SIZE];
    let mut names = ShortNameGenerator::new("x.txt");
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"X       TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"X~1     TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"X~2     TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"X~3     TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"X~4     TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"X40DA~1 TXT");
    names.add_existing(&buf);
    buf = names.generate().unwrap();
    assert_eq!(&buf, b"X40DA~2 TXT");
}
