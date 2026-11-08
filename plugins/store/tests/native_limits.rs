//! The limits of the native rules hold for text nobody reviewed: a line, a
//! name or a summary past its cap is not read, the runs and a section stop
//! growing, and none of it panics.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the tests")]
use paperpaw_plugin_store::native_model as model;

fn line(name: &str, summary: &str) -> String {
    format!("5\t10\t1\t20260101\t20260202\tid\t1.0.0\t0.4\tauthor\tMIT\t{name}\t{summary}")
}

#[test]
fn a_line_within_the_caps_is_read() {
    let name = "n".repeat(model::MAX_NAME);
    let summary = "s".repeat(model::MAX_SUMMARY);
    let text = line(&name, &summary);
    assert!(text.len() <= model::MAX_LINE);
    let entry = model::Entry::parse(&text).unwrap().unwrap();
    assert_eq!(entry.name, name);
    assert_eq!(entry.summary, summary);
}

#[test]
fn a_name_or_summary_past_its_cap_is_not_read() {
    let long_name = "n".repeat(model::MAX_NAME + 1);
    assert!(
        model::Entry::parse(&line(&long_name, "s"))
            .unwrap()
            .is_none()
    );
    let long_summary = "s".repeat(model::MAX_SUMMARY + 1);
    assert!(
        model::Entry::parse(&line("n", &long_summary))
            .unwrap()
            .is_none()
    );
    // Every other text is held to the name's cap.
    let long = "x".repeat(model::MAX_NAME + 1);
    let fields = [5, 6, 7, 8, 9];
    for at in fields {
        let mut parts: Vec<String> = line("n", "s").split('\t').map(String::from).collect();
        parts[at] = long.clone();
        assert!(
            model::Entry::parse(&parts.join("\t")).unwrap().is_none(),
            "field {at}"
        );
    }
}

#[test]
fn a_line_past_the_line_cap_is_not_read() {
    // Within each text's cap but past the line's: nine fields at their caps
    // cannot be a line the store wrote.
    let name = "n".repeat(model::MAX_NAME);
    let big = "b".repeat(model::MAX_LINE);
    let text = line(&name, &big);
    assert!(text.len() > model::MAX_LINE);
    assert!(model::Entry::parse(&text).unwrap().is_none());
}

#[test]
fn text_of_any_length_is_read_or_refused_without_a_panic() {
    for n in [0, 1, 79, 80, 81, 299, 300, 301, 1023, 1024, 1025, 100_000] {
        let text = "é".repeat(n);
        let _ = model::Entry::parse(&text).unwrap();
        let _ = model::Entry::parse(&line(&text, &text)).unwrap();
    }
}

#[test]
fn runs_stop_at_their_cap() {
    let mut runs = Vec::new();
    for group in 0..model::MAX_RUNS * 3 {
        model::add_to_run(&mut runs, group).unwrap();
    }
    assert_eq!(runs.len(), model::MAX_RUNS);
    // The rest went on the last run.
    let total: u32 = runs.iter().map(|(_, n)| n).sum();
    assert_eq!(usize::try_from(total).unwrap(), model::MAX_RUNS * 3);
    // Plugins of one group in a row make one run.
    let mut runs = Vec::new();
    for _ in 0..5 {
        model::add_to_run(&mut runs, 3).unwrap();
    }
    assert_eq!(runs, [(3, 5)]);
}

#[test]
fn a_section_keeps_plugin_names_up_to_its_cap() {
    let mut ids = Vec::new();
    assert!(!model::keep_in_section(&mut ids, "Not A Name").unwrap());
    assert!(!model::keep_in_section(&mut ids, "").unwrap());
    for n in 0..model::MAX_SECTION + 5 {
        let kept = model::keep_in_section(&mut ids, &format!("plugin-{n}")).unwrap();
        assert_eq!(kept, n < model::MAX_SECTION, "plugin {n}");
    }
    assert_eq!(ids.len(), model::MAX_SECTION);
}
