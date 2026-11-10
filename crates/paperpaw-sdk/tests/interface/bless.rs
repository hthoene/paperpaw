//! The bless gate of the plugin interface ([`super::bless_refused`],
//! [`super::fingerprint_drift`]): raises, drafts and fingerprints.

use super::*;

/// What `interface.txt` keeps at interface `n`, its fingerprint made on
/// `arch` as `fingerprint`.
fn kept_at(n: u16, fingerprint: Option<(&str, u64)>) -> String {
    let mut kept = format!("interface = {n}\nlinker = b979ebf89df9ea9f\n");
    if let Some((arch, value)) = fingerprint {
        kept.push_str(&format!("fingerprint = {arch} {}\n", Fingerprint(value)));
    }
    kept
}

/// The bless of #363 as it was first made: the shapes changed (the
/// `Screen` trait's items moved), `interface.txt` lowered from 5 to 4 by
/// hand so that INTERFACE 5 would pass, and the fingerprint stayed
/// 00053e32940609b9. Refused now, on the architecture it was kept on and
/// on any other.
#[test]
fn a_breaking_bless_that_keeps_the_fingerprint_is_refused_whatever_interface_txt_says() {
    let fingerprint = 0x0005_3e32_9406_09b9;
    for arch in ["riscv32", "x86_64", "aarch64"] {
        let kept = kept_at(4, Some(("riscv32", fingerprint)));
        let refused = bless_refused(&kept, true, Fingerprint(fingerprint), 0, arch);
        assert!(
            refused.is_some_and(|r| r.contains("made at interface 5")),
            "{arch}"
        );
    }
    // As the number says, too.
    let kept = kept_at(5, Some(("x86_64", fingerprint)));
    assert!(bless_refused(&kept, true, Fingerprint(fingerprint), 0, "x86_64").is_some());
}

/// The raise that was made instead, 5 to 6, is a bless; so is a change
/// plugins built before do not see (an item appended to a growing trait,
/// a new type), at the interface and fingerprint kept.
#[test]
fn a_raise_and_what_plugins_built_before_do_not_see_are_blessed() {
    let five = Some(("x86_64", 0x0005_3e32_9406_09b9));
    let six = Fingerprint(0x0006_3e32_9406_09b9);
    assert_eq!(
        bless_refused(&kept_at(5, five), true, six, 0, "x86_64"),
        None
    );
    assert_eq!(
        bless_refused(&kept_at(5, five), true, six, 0, "aarch64"),
        None
    );
    let kept = kept_at(6, Some(("x86_64", six.0)));
    assert_eq!(bless_refused(&kept, false, six, 0, "x86_64"), None);
    // Kept before the fingerprint was: the number alone, as before.
    assert_eq!(
        bless_refused(&kept_at(5, None), true, six, 0, "x86_64"),
        None
    );
    assert!(bless_refused(&kept_at(6, None), true, six, 0, "x86_64").is_some());
    assert_eq!(
        bless_refused(&kept_at(6, None), false, six, 0, "x86_64"),
        None
    );
}

/// What `interface.txt` keeps at draft `draft` of interface `n`.
fn kept_draft(n: u16, draft: u16, fingerprint: u64) -> String {
    let kept = kept_at(n, Some(("x86_64", fingerprint)));
    kept.replacen('\n', &format!("\ndraft = {draft}\n"), 1)
}

/// A draft changes what plugins rely on at the draft kept (one interface 7
/// until 1.0), never moves back; a frozen interface never becomes a draft
/// again and changes only with a raise. A raise may start at a draft.
#[test]
fn a_draft_changes_in_place_and_a_frozen_one_never() {
    let one = 0x0007_0000_0000_0001;
    let now = Fingerprint(0x0007_0000_0000_0002);
    let at_one = kept_draft(7, 1, one);
    assert!(at_one.contains("interface = 7\ndraft = 1\n"), "{at_one}");
    assert_eq!(bless_refused(&at_one, true, now, 2, "x86_64"), None);
    assert_eq!(bless_refused(&at_one, true, now, 0, "x86_64"), None);
    // A change plugins rely on, or a layout, at the draft kept.
    assert_eq!(
        bless_refused(&at_one, true, Fingerprint(one), 1, "x86_64"),
        None
    );
    assert_eq!(bless_refused(&at_one, false, now, 1, "x86_64"), None);
    assert!(bless_refused(&kept_draft(7, 3, one), true, now, 2, "x86_64").is_some());
    // At a frozen interface the message names the raise.
    let frozen = bless_refused(&kept_draft(7, 0, one), true, now, 0, "x86_64").unwrap();
    assert!(
        frozen.contains("raise native::INTERFACE (now 7)"),
        "{frozen}"
    );
    // What plugins do not see changes at the draft kept, as at any interface.
    assert_eq!(
        bless_refused(&at_one, false, Fingerprint(one), 1, "x86_64"),
        None
    );
    // Frozen (no draft kept, or draft 0): only a raise.
    for frozen in [kept_at(7, Some(("x86_64", one))), kept_draft(7, 0, one)] {
        assert!(bless_refused(&frozen, true, now, 1, "x86_64").is_some());
        assert!(bless_refused(&frozen, true, now, 0, "x86_64").is_some());
        let eight = Fingerprint(0x0008_0000_0000_0001);
        assert_eq!(bless_refused(&frozen, true, eight, 1, "x86_64"), None);
    }
    // Frozen, interface.txt keeps no draft line, as before there were drafts.
    assert_eq!(
        (draft_line(0), draft_line(2)),
        (String::new(), "draft = 2\n".into())
    );
    // The raise of interface 6 to the first draft of 7.
    let six = kept_at(6, Some(("x86_64", 0x0006_3e32_9406_09b9)));
    assert_eq!(
        bless_refused(&six, true, Fingerprint(one), 1, "x86_64"),
        None
    );
}

/// Layouts of what crosses that changed while the shapes did not (a
/// size): a raise, where the fingerprint kept was made on this
/// architecture; on another the two cannot be told apart.
#[test]
fn layouts_changed_at_the_interface_kept_are_a_raise() {
    let kept = kept_at(6, Some(("x86_64", 0x0006_0000_0000_0001)));
    let now = Fingerprint(0x0006_0000_0000_0002);
    assert!(bless_refused(&kept, false, now, 0, "x86_64").is_some());
    assert_eq!(bless_refused(&kept, false, now, 0, "aarch64"), None);
    assert!(fingerprint_drift(&kept, now, "x86_64").is_some());
    assert_eq!(fingerprint_drift(&kept, now, "aarch64"), None);
    // The outline feature adds layouts: a build with it is another kind.
    assert_eq!(bless_refused(&kept, false, now, 0, "x86_64+outline"), None);
    assert_eq!(fingerprint_drift(&kept, now, "x86_64+outline"), None);
    // Another interface than the one kept: on any architecture.
    assert!(fingerprint_drift(&kept, Fingerprint(0x0007_0000_0000_0001), "aarch64").is_some());
    assert_eq!(
        fingerprint_drift(&kept, Fingerprint(0x0006_0000_0000_0001), "x86_64"),
        None
    );
}

/// The fingerprint line is read back as written, and left out where the
/// rest is compared.
#[test]
fn the_fingerprint_kept_is_read_back() {
    let kept = format!("interface = {INTERFACE}\n{}", fingerprint_line());
    let (arch, fingerprint) = kept_fingerprint(&kept).unwrap();
    assert_eq!(
        (arch.as_str(), fingerprint),
        (made_on().as_str(), FINGERPRINT)
    );
    assert_eq!(
        without_fingerprint(&kept),
        format!("interface = {INTERFACE}\n")
    );
    assert_eq!(kept_fingerprint("interface = 6\n"), None);
}
