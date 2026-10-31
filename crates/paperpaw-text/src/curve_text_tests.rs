use alloc::format;
use alloc::string::String;
use proptest::prelude::*;

use crate::ToneCurve;

const HANI: &str = "0 20 40 60 80 100 120 140 160 180 200 220 230 240 250 255";

fn text(tone: &ToneCurve) -> String {
    let mut out = String::new();
    tone.to_text(&mut out);
    out
}

#[test]
fn one_curve_reads_its_names_and_numbers() {
    assert_eq!(ToneCurve::parse("smooth"), Some(ToneCurve::SMOOTH));
    assert_eq!(ToneCurve::parse(" Smooth "), Some(ToneCurve::SMOOTH));
    assert_eq!(
        ToneCurve::parse("contrast:-1"),
        Some(ToneCurve::contrast(-1))
    );
    assert_eq!(
        ToneCurve::parse("contrast: 2"),
        Some(ToneCurve::contrast(2))
    );
    assert_eq!(
        ToneCurve::parse("sharp:128"),
        Some(ToneCurve::SMOOTH.sharp(128))
    );
    let tone = ToneCurve::parse(HANI).unwrap();
    assert_eq!((tone.alpha(0), tone.alpha(1), tone.alpha(15)), (0, 20, 255));
    assert_eq!(ToneCurve::parse(&HANI.replace(' ', ",")), Some(tone));
}

#[test]
fn level_0_is_forced_empty() {
    let tone =
        ToneCurve::parse("9 20 40 60 80 100 120 140 160 180 200 220 230 240 250 255").unwrap();
    assert_eq!(tone.alpha(0), 0);
}

#[test]
fn anything_else_is_none() {
    for bad in [
        "",
        " ",
        "bright",
        "contrast",
        "contrast:3",
        "contrast:-2",
        "contrast:x",
        "sharp:0",
        "sharp:256",
        "sharp:",
        "1 2 3",
        "0 20 40 60 80 100 120 140 160 180 200 220 230 240 250",
        "0 20 40 60 80 100 120 140 160 180 200 220 230 240 250 255 255",
        "0 20 40 60 80 100 120 140 160 180 200 220 230 240 250 256",
        "0 20 40 60 80 100 120 140 160 180 200 220 230 240 250 -1",
        "arab=smooth",
        "a;b",
    ] {
        assert_eq!(ToneCurve::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn a_curve_is_written_as_short_as_it_can_be() {
    assert_eq!(text(&ToneCurve::SMOOTH), "smooth");
    assert_eq!(text(&ToneCurve::contrast(-1)), "contrast:-1");
    assert_eq!(text(&ToneCurve::contrast(2)), "contrast:2");
    // Thresholds 86 to 102 make one curve: the lowest names it.
    assert_eq!(text(&ToneCurve::SMOOTH.sharp(96)), "sharp:86");
    assert_eq!(
        ToneCurve::parse("sharp:86"),
        Some(ToneCurve::SMOOTH.sharp(96))
    );
    assert_eq!(
        text(&ToneCurve::parse(HANI).unwrap()),
        "0 20 40 60 80 100 120 140 160 180 200 220 230 240 250 255"
    );
}

#[test]
fn a_setting_picks_the_entry_of_the_script_or_the_unscripted_one() {
    let value = format!("smooth; arab=contrast:1; hani={HANI}");
    let arab = Some(ToneCurve::contrast(1));
    assert_eq!(ToneCurve::parse_for(&value, None), Some(ToneCurve::SMOOTH));
    assert_eq!(ToneCurve::parse_for(&value, Some("Arab")), arab);
    assert_eq!(ToneCurve::parse_for(&value, Some("arab")), arab);
    assert_eq!(ToneCurve::parse_for(&value, Some("ARAB")), arab);
    assert_eq!(
        ToneCurve::parse_for(&value, Some("Hani")),
        ToneCurve::parse(HANI)
    );
    assert_eq!(
        ToneCurve::parse_for(&value, Some("Grek")),
        Some(ToneCurve::SMOOTH)
    );
    // The scripted entry wins wherever it stands.
    assert_eq!(
        ToneCurve::parse_for("arab=contrast:1; smooth", Some("Arab")),
        arab
    );
}

#[test]
fn only_scripted_entries_mean_stock_for_a_reader_without_a_script() {
    assert_eq!(ToneCurve::parse_for("arab=contrast:1", None), None);
    assert_eq!(ToneCurve::parse_for("arab=contrast:1", Some("Latn")), None);
    assert_eq!(ToneCurve::parse_for("", Some("Arab")), None);
}

#[test]
fn an_entry_that_does_not_parse_is_skipped() {
    // A bad scripted entry falls through to the unscripted one.
    let value = "contrast:1; arab=bright";
    assert_eq!(
        ToneCurve::parse_for(value, Some("Arab")),
        Some(ToneCurve::contrast(1))
    );
    // A bad unscripted entry does not stop a later one.
    assert_eq!(
        ToneCurve::parse_for("bright; sharp:100", None),
        Some(ToneCurve::SMOOTH.sharp(100))
    );
    // Nothing usable: stock.
    assert_eq!(
        ToneCurve::parse_for("bright; arab=also bad", Some("Arab")),
        None
    );
}

#[test]
fn keys_that_are_not_four_letters_are_ignored() {
    for value in [
        "ui=contrast:2",
        "size>20=contrast:2",
        "ar=contrast:2",
        "arabic=contrast:2",
        "ar4b=contrast:2",
        "=contrast:2",
    ] {
        assert_eq!(ToneCurve::parse_for(value, Some("Arab")), None, "{value}");
        assert_eq!(
            ToneCurve::parse_for(&format!("{value}; sharp:9"), None),
            Some(ToneCurve::SMOOTH.sharp(9))
        );
    }
}

#[test]
fn empty_entries_and_spaces_are_ignored() {
    assert_eq!(
        ToneCurve::parse_for(" ;; smooth ;", None),
        Some(ToneCurve::SMOOTH)
    );
    assert_eq!(
        ToneCurve::parse_for(" arab = contrast:1 ", Some("Arab")),
        Some(ToneCurve::contrast(1))
    );
}

proptest! {
    #[test]
    fn parse_and_to_text_round_trip_any_table(alphas in proptest::array::uniform16(any::<u8>())) {
        let tone = ToneCurve::from_alphas(alphas);
        let written = text(&tone);
        prop_assert_eq!(ToneCurve::parse(&written), Some(tone));
        prop_assert_eq!(ToneCurve::parse_for(&written, None), Some(tone));
    }

    #[test]
    fn nothing_reads_as_a_panic(input in ".{0,300}", script in proptest::option::of(".{0,6}")) {
        let _ = ToneCurve::parse(&input);
        let _ = ToneCurve::parse_for(&input, script.as_deref());
    }

    #[test]
    fn list_shaped_noise_does_not_panic(input in "[a-zA-Z0-9=;:, \\-]{0,200}", script in "[A-Za-z]{0,5}") {
        let _ = ToneCurve::parse_for(&input, Some(&script));
    }
}
