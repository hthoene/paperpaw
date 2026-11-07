#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]
use super::*;
use alloc::string::ToString;

/// A vault in memory: counts the letters it hands out so salts differ.
#[derive(Default)]
struct Mem {
    kept: Option<String>,
    drawn: u32,
}

impl Vault for Mem {
    fn get(&mut self) -> Result<Option<String>, Error> {
        self.kept.as_deref().map(copy).transpose()
    }
    fn keep(&mut self, value: &str) -> Result<(), Error> {
        self.kept = Some(copy(value)?);
        Ok(())
    }
    fn remove(&mut self) -> Result<(), Error> {
        self.kept = None;
        Ok(())
    }
    fn token(&mut self, count: usize) -> Result<String, Error> {
        self.drawn += 1;
        let mut text = String::new();
        text.try_reserve(count).map_err(|_| Error::NoMemory)?;
        for at in 0..count {
            // Different letters for each draw.
            text.push(char::from(b'a' + ((self.drawn as usize + at) % 26) as u8));
        }
        Ok(text)
    }
}

fn with_code(kind: &str, code: &str) -> Mem {
    let mut vault = Mem::default();
    keep(&mut vault, kind, code).unwrap();
    vault
}

#[test]
fn hashes_depend_on_salt_and_code() {
    let a = hash("salt", "1234");
    assert_eq!(a.len(), 40);
    assert_eq!(a, hash("salt", "1234"));
    assert_ne!(a, hash("pepper", "1234"));
    assert_ne!(a, hash("salt", "1235"));
}

#[test]
fn a_code_kept_is_the_code_checked_and_never_stored_as_it_is() {
    let mut vault = with_code("pin", "1234");
    let stored = vault.kept.clone().unwrap();
    assert!(!stored.contains("1234"));
    assert!(stored.starts_with("pin:v2."));
    assert!(check(&mut vault, "1234").unwrap());
    assert!(!check(&mut vault, "1235").unwrap());
    assert_eq!(kept_kind(&mut vault).unwrap().as_deref(), Some("pin"));
}

#[test]
fn salts_are_not_made_of_the_code() {
    let mut a = Mem::default();
    keep(&mut a, "pin", "1234").unwrap();
    keep(&mut a, "pin", "1234").unwrap();
    let mut b = Mem::default();
    keep(&mut b, "pin", "1234").unwrap();
    // The second salt of one vault differs from the first of another.
    assert_ne!(a.kept, b.kept);
}

#[test]
fn a_damaged_or_empty_code_locks_nothing() {
    for stored in [
        "",
        "pin",
        "pin:salt",
        "pin::hash",
        "pin:salt:",
        "off:s:h",
        "other:s:h",
    ] {
        let mut vault = Mem {
            kept: Some(stored.to_string()),
            drawn: 0,
        };
        assert_eq!(kept_kind(&mut vault).unwrap(), None, "{stored:?}");
        assert_eq!(active(&mut vault, "lock.kind.pin").unwrap(), None);
    }
    assert_eq!(kept_kind(&mut Mem::default()).unwrap(), None);
    assert!(!check(&mut Mem::default(), "1234").unwrap());
}

#[test]
fn the_kind_in_use_is_the_kind_kept_and_named_by_the_setting() {
    let mut vault = with_code("pattern", "0124");
    assert_eq!(
        active(&mut vault, "lock.kind.pattern").unwrap().as_deref(),
        Some("pattern")
    );
    assert_eq!(active(&mut vault, "lock.kind.pin").unwrap(), None);
    assert_eq!(active(&mut vault, "lock.kind.off").unwrap(), None);
    assert_eq!(
        active(&mut vault, "pattern").unwrap().as_deref(),
        Some("pattern")
    );
}

#[test]
fn a_code_kept_with_a_salt_made_of_it_is_kept_anew_at_random() {
    let old = "pin:oldsalt:".to_string() + &hash("oldsalt", "1234");
    let mut vault = Mem {
        kept: Some(old.clone()),
        drawn: 0,
    };
    renew(&mut vault, "9999").unwrap();
    assert_eq!(
        vault.kept.as_deref(),
        Some(old.as_str()),
        "wrong code: kept as it was"
    );
    renew(&mut vault, "1234").unwrap();
    assert!(vault.kept.as_deref().unwrap().starts_with("pin:v2."));
    assert!(check(&mut vault, "1234").unwrap());
    let renewed = vault.kept.clone();
    renew(&mut vault, "1234").unwrap();
    assert_eq!(vault.kept, renewed, "a random salt stays");
}

#[test]
fn five_wrong_tries_make_the_next_one_wait_half_a_minute() {
    let mut tries = Tries::default();
    for _ in 0..4 {
        assert_eq!(tries.failed(1000), None);
        assert!(!tries.waiting(1000));
    }
    assert_eq!(tries.failed(1000), Some(1000 + WAIT_MS));
    assert!(tries.waiting(1000 + WAIT_MS - 1));
    assert!(!tries.waiting(1000 + WAIT_MS));
    assert!(tries.succeeded());
    assert_eq!(tries, Tries::default());
    assert!(!tries.succeeded());
}

#[test]
fn a_wait_due_when_paperpaw_stopped_is_waited_anew() {
    assert_eq!(Tries::restored(3, 500).wait_until, None);
    assert_eq!(Tries::restored(5, 500).wait_until, Some(500 + WAIT_MS));
    assert_eq!(Tries::restored(200, u64::MAX).wait_until, Some(u64::MAX));
}

#[test]
fn the_count_on_the_card_reads_as_a_number() {
    assert_eq!(parse_tries(b"3\n"), 3);
    assert_eq!(parse_tries(b" 5 "), 5);
    assert_eq!(parse_tries(b"x"), 0);
    assert_eq!(parse_tries(b"999"), 0);
    assert_eq!(parse_tries(&[0xff, 0xfe]), 0);
    assert_eq!(parse_tries(b""), 0);
}

fn unlock(code: &str) -> (Machine, Mem, Tries) {
    let vault = with_code("pin", code);
    (
        Machine::new(true, Some("pin".to_string())).unwrap(),
        vault,
        Tries::default(),
    )
}

#[test]
fn the_right_code_unlocks_and_the_wrong_one_is_counted_and_kept() {
    let (mut m, mut vault, mut tries) = unlock("1234");
    let out = m
        .submit("0000".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!(out, [Do::KeepTries]);
    assert_eq!(m.note, Some(Note::Wrong));
    assert_eq!(tries.failures, 1);
    let out = m
        .submit("1234".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!(out, [Do::KeepTries, Do::Unlock], "the tries forgotten");
    assert_eq!(tries.failures, 0);
}

#[test]
fn during_a_wait_not_even_the_right_code_is_tried() {
    let (mut m, mut vault, mut tries) = unlock("1234");
    let mut last = Vec::new();
    for _ in 0..5 {
        last = m
            .submit("0000".to_string(), &mut vault, &mut tries, 100, false)
            .unwrap();
    }
    assert_eq!(last, [Do::WaitUntil(100 + WAIT_MS), Do::KeepTries]);
    let out = m
        .submit("1234".to_string(), &mut vault, &mut tries, 200, false)
        .unwrap();
    assert!(out.is_empty());
    let out = m
        .submit(
            "1234".to_string(),
            &mut vault,
            &mut tries,
            100 + WAIT_MS,
            false,
        )
        .unwrap();
    assert!(out.contains(&Do::Unlock));
}

#[test]
fn with_no_code_kept_any_more_the_device_is_let_through() {
    let mut m = Machine::new(true, None).unwrap();
    let out = m
        .submit(
            String::new(),
            &mut Mem::default(),
            &mut Tries::default(),
            0,
            true,
        )
        .unwrap();
    assert_eq!(out, [Do::Unlock]);
}

#[test]
fn setting_up_asks_the_code_in_use_then_the_kind_then_the_new_code_twice() {
    let (mut m, mut vault, mut tries) = unlock("1234");
    m.step = Step::Verify;
    m.submit("1234".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!(m.step, Step::Choose);
    assert!(m.choose_kind("pin", &mut vault).unwrap().is_empty());
    assert_eq!(m.step, Step::New);
    m.submit("12".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!((m.step.clone(), m.note), (Step::New, Some(Note::TooShort)));
    m.submit("5678".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!(m.step, Step::Confirm("5678".to_string()));
    m.submit("5679".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!((m.step.clone(), m.note), (Step::New, Some(Note::Mismatch)));
    m.submit("5678".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    let out = m
        .submit("5678".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!(out, [Do::Done]);
    assert!(check(&mut vault, "5678").unwrap());
    assert!(!check(&mut vault, "1234").unwrap());
}

#[test]
fn off_forgets_the_code_at_once() {
    let (mut m, mut vault, _) = unlock("1234");
    m.step = Step::Choose;
    assert_eq!(m.choose_kind("off", &mut vault).unwrap(), [Do::Removed]);
    assert_eq!(vault.kept, None);
}

#[test]
fn a_password_is_asked_for_on_the_keyboard_once_per_step() {
    let mut vault = Mem::default();
    let mut tries = Tries::default();
    let mut m = Machine::new(false, None).unwrap();
    assert_eq!(m.step, Step::Choose);
    assert_eq!(
        m.choose_kind("password", &mut vault).unwrap(),
        [Do::AskPassword]
    );
    let out = m
        .submit("hunter22".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!(out, [Do::AskPassword], "asked again for the same");
    let out = m
        .submit("hunter22".to_string(), &mut vault, &mut tries, 0, false)
        .unwrap();
    assert_eq!(out, [Do::Done]);
    assert_eq!(kept_kind(&mut vault).unwrap().as_deref(), Some("password"));
}

#[test]
fn nothing_fails_open_when_memory_runs_out() {
    crate::fail_alloc::sweep(|| {
        let mut vault = Mem::default();
        keep(&mut vault, "pin", "1234")?;
        check(&mut vault, "1234").map(|ok| ok as usize)
    });
    crate::fail_alloc::sweep(|| {
        let mut vault = Mem::default();
        keep(&mut vault, "pin", "1234")?;
        let mut m = Machine::new(true, Some(copy("pin")?))?;
        let mut tries = Tries::default();
        m.submit(copy("0000")?, &mut vault, &mut tries, 0, false)
            .map(|out| out.len())
    });
}
