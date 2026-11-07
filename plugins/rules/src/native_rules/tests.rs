#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]
use super::engine::{self, Act, Battery, Chain, Facts, MAX_CHAIN, Value};
use super::rule::{self, *};
use alloc::{string::String, vec, vec::Vec};
use paperpaw_sdk::native::Error;

const NIGHT: &str = "core.night-mode";

fn s(text: &str) -> String {
    String::from(text)
}

fn every_kind() -> Vec<Rule> {
    [
        Trigger::Time {
            minute: 22 * 60,
            days: EVERY_DAY,
        },
        Trigger::Time {
            minute: 7 * 60,
            days: WEEKDAYS,
        },
        Trigger::Start,
        Trigger::Every(30),
        Trigger::BookOpened,
        Trigger::BookClosed,
        Trigger::BookFinished,
        Trigger::Resume,
        Trigger::Sleep,
        Trigger::BatteryBelow(20),
        Trigger::BatteryAtLeast(80),
        Trigger::Charging,
        Trigger::Unplugged,
        Trigger::SwitchOn(s(NIGHT)),
        Trigger::SwitchOff(s(NIGHT)),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, trigger)| Rule {
        trigger,
        effect: match i % 4 {
            0 => Effect::Switch(s(NIGHT), true),
            1 => Effect::Action(s("opds.sync")),
            2 => Effect::Number(s("core.frontlight"), -3),
            _ => Effect::Message(s("Time to sleep")),
        },
        on: i % 3 != 0,
        condition: match i % 7 {
            0 => Condition::Always,
            1 => Condition::Between {
                from: 22 * 60,
                to: 6 * 60,
            },
            2 => Condition::Reading,
            3 => Condition::NotReading,
            4 => Condition::OnBattery,
            5 => Condition::Charging,
            _ => Condition::SwitchOff(s(NIGHT)),
        },
    })
    .collect()
}

#[test]
fn rules_are_kept() {
    let rules = every_kind();
    let bytes = encode_within(&rules, &[], MAX_FILE).unwrap();
    assert_eq!(decode(&bytes).unwrap(), rules);
    assert!(unread_lines(&bytes).unwrap().is_empty());
    // Numbers at the ends of their range.
    for v in [0, 1, -1, i64::MIN, i64::MAX] {
        let rule = Rule {
            trigger: Trigger::Start,
            effect: Effect::Number(s("core.x"), v),
            on: true,
            condition: Condition::Always,
        };
        let bytes = encode_within(core::slice::from_ref(&rule), &[], MAX_FILE).unwrap();
        assert_eq!(decode(&bytes).unwrap(), [rule]);
    }
}

#[test]
fn what_it_writes_is_what_interface_6_wrote() {
    let rules = decode(include_bytes!("../../testdata/rules.txt.v1")).unwrap();
    assert_eq!(rules.len(), 5);
    assert_eq!(
        rules[0].trigger,
        Trigger::Time {
            minute: 22 * 60,
            days: EVERY_DAY
        }
    );
    assert_eq!(rules[0].effect, Effect::Switch(s(NIGHT), true));
    assert_eq!(rules[0].condition, Condition::Always);
    assert_eq!(
        rules[1].trigger,
        Trigger::Time {
            minute: 7 * 60,
            days: WEEKDAYS
        }
    );
    assert_eq!(rules[1].effect, Effect::Switch(s(NIGHT), false));
    assert_eq!(rules[2].effect, Effect::Message(s("Battery low")));
    assert!(!rules[2].on);
    assert_eq!(rules[2].condition, Condition::NotReading);
    assert_eq!(rules[3].effect, Effect::Number(s("core.font-size"), 12));
    assert_eq!(
        rules[3].condition,
        Condition::Between {
            from: 8 * 60,
            to: 17 * 60
        }
    );
    assert_eq!(rules[4].trigger, Trigger::BatteryAtLeast(80));
    assert!(!rules[4].on);
    assert_eq!(rules[4].condition, Condition::Charging);
    // Written again: the lines of the file, in its words.
    let bytes = encode_within(&rules, &[], MAX_FILE).unwrap();
    let text = core::str::from_utf8(&bytes).unwrap();
    assert!(text.starts_with("time 22:00\tswitch-on\tcore.night-mode\ton\n"));
    assert!(text.contains("time 07:00 62\tswitch-off\tcore.night-mode\ton\n"));
    assert!(text.contains("book\tset\tcore.font-size=12\ton\tbetween 08:00 17:00\n"));
    assert!(
        text.contains("battery-at 80\tmessage\tUnplug now to spare the battery\toff\tcharging\n")
    );
}

#[test]
fn words_that_are_not_read_are_none() {
    assert_eq!(Trigger::parse("battery-below 0"), Ok(None));
    assert_eq!(Trigger::parse("battery-at 101"), Ok(None));
    assert_eq!(
        Trigger::parse("battery-at 80 %"),
        Ok(Some(Trigger::BatteryAtLeast(80)))
    );
    assert_eq!(Trigger::parse("every 0"), Ok(None));
    assert_eq!(Trigger::parse("time 22:00 0"), Ok(None));
    assert_eq!(Trigger::parse("time 25:00"), Ok(None));
    assert_eq!(Trigger::parse("switch-on Bad Id"), Ok(None));
    assert_eq!(Trigger::parse("moon full"), Ok(None));
    assert_eq!(Condition::parse(""), Ok(Some(Condition::Always)));
    assert_eq!(Condition::parse("between 22:00"), Ok(None));
    assert_eq!(Condition::parse("if-raining"), Ok(None));
    assert_eq!(
        Trigger::parse("time 22:00"),
        Ok(Some(Trigger::Time {
            minute: 1320,
            days: EVERY_DAY
        }))
    );
}

#[test]
fn a_tab_in_a_message_does_not_split_the_line() {
    let rule = Rule {
        trigger: Trigger::Start,
        effect: Effect::Message(s("a\tb\nc")),
        on: true,
        condition: Condition::SwitchOn(s(NIGHT)),
    };
    let bytes = encode_within(core::slice::from_ref(&rule), &[], MAX_FILE).unwrap();
    let back = decode(&bytes).unwrap();
    assert_eq!(back[0].effect, Effect::Message(s("a b c")));
    assert_eq!(back[0].condition, Condition::SwitchOn(s(NIGHT)));
}

/// Lines this version does not read (a newer version's, or damaged) are
/// written back as they are, even when all the rules are removed.
#[test]
fn lines_it_cannot_read_are_kept() {
    let newer = "moon full\tmessage\tLook up\ton";
    let condition = "start\tmessage\tHello\ton\tif-raining";
    let broken: &[u8] = b"time 25:00\tswitch-on\tcore.night-mode\ton\t\xff";
    let mut file = include_bytes!("../../testdata/rules.txt.v1").to_vec();
    for line in [newer.as_bytes(), condition.as_bytes(), broken] {
        file.extend_from_slice(line);
        file.push(b'\n');
    }
    let mut rules = decode(&file).unwrap();
    assert_eq!(rules.len(), 5);
    let unread = unread_lines(&file).unwrap();
    rules.remove(0);
    let bytes = encode_within(&rules, &unread, MAX_FILE).unwrap();
    assert_eq!(decode(&bytes).unwrap().len(), 4);
    let bytes = encode_within(&[], &unread, MAX_FILE).unwrap();
    let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
    for line in [newer.as_bytes(), condition.as_bytes(), broken] {
        assert!(lines.contains(&line));
    }
    // Invalid UTF-8 on a line costs only that line.
    assert_eq!(
        decode(b"\xff\xfe\nstart\tmessage\tHi\ton\n").unwrap().len(),
        1
    );
}

#[test]
fn the_limit_is_checked_before_a_file_is_made() {
    let rules = every_kind();
    let size = encode_within(&rules, &[], usize::MAX).unwrap().len();
    assert_eq!(encode_within(&rules, &[], size).unwrap().len(), size);
    assert_eq!(encode_within(&rules, &[], size - 1), Err(Error::NoSpace));
    assert_eq!(
        encode_within(&rules, &vec![b'x'; size], size),
        Err(Error::NoSpace)
    );
}

#[test]
fn allocations_are_fallible_and_the_next_try_works() {
    let rules = every_kind();
    let bytes = encode_within(&rules, &[], MAX_FILE).unwrap();
    crate::fail_alloc::sweep(|| encode_within(&rules, b"x\n", MAX_FILE).map(|b| b.len()));
    crate::fail_alloc::sweep(|| decode(&bytes).map(|r| r.len()));
    crate::fail_alloc::sweep(|| unread_lines(&bytes).map(|b| b.len()));
    crate::fail_alloc::sweep(|| copy_rules(&rules).map(|r| r.len()));
    crate::fail_alloc::sweep(|| rules[0].try_clone().map(|r| r.on));
    crate::fail_alloc::sweep(|| Trigger::parse("switch-on core.night-mode").map(|t| t.is_some()));
}

#[test]
fn times_and_levels() {
    assert!(between(23 * 60, 22 * 60, 6 * 60));
    assert!(between(60, 22 * 60, 6 * 60));
    assert!(!between(12 * 60, 22 * 60, 6 * 60));
    assert!(between(9 * 60, 8 * 60, 17 * 60));
    assert!(!between(17 * 60, 8 * 60, 17 * 60));
    assert_eq!(parse_time("22:00"), Some(1320));
    assert_eq!(parse_time(" 7.05 "), Some(425));
    assert_eq!(parse_time("7"), Some(420));
    assert_eq!(parse_time("24:00"), None);
    assert_eq!(parse_time("abc"), None);
    let mut buf = [0; 5];
    assert_eq!(clock(425, &mut buf), "07:05");
    assert_eq!(clock(u16::MAX, &mut buf).len(), 5);
    assert_eq!(minutes_until(1300, 1320), 20);
    assert_eq!(minutes_until(1330, 1320), 24 * 60 - 10);
    assert_eq!(minutes_until(1320, 1320), 24 * 60);
    assert_eq!(parse_percent(" 80 % "), Some(80));
    assert_eq!(parse_percent("0"), None);

    let mut levels = Levels::up_to(75);
    assert!(levels.contains(75) && !levels.contains(76));
    let new = levels.reach(81);
    assert!((76..=81).all(|p| new.contains(p)));
    assert!(!new.contains(75) && !new.contains(82));
    assert_eq!(levels.reach(81), Levels::default());
    assert_eq!(levels.reach(79), Levels::default());
    assert_eq!(levels.reach(81), Levels::default());
    assert_eq!(levels.reach(77), Levels::default());
    assert!(levels.reach(80).contains(80));
    assert!(Levels::up_to(100).contains(100));
    assert!(Levels::up_to(0).contains(0) && !Levels::up_to(0).contains(1));
    assert!(!Levels::up_to(255).contains(200));
}

fn rule(trigger: Trigger, effect: Effect, condition: Condition) -> Rule {
    Rule {
        trigger,
        effect,
        on: true,
        condition,
    }
}

fn facts<'a>(
    setting: &'a dyn Fn(&str) -> Result<Option<Value>, Error>,
    reading: bool,
) -> Facts<'a> {
    Facts {
        reading,
        charging: Some(false),
        minute: Some(23 * 60),
        setting,
    }
}

#[test]
fn a_rule_sets_off_what_it_asks_once() {
    let rules = [
        rule(
            Trigger::Start,
            Effect::Switch(s(NIGHT), true),
            Condition::Always,
        ),
        rule(
            Trigger::Start,
            Effect::Action(s("opds.sync")),
            Condition::Always,
        ),
        rule(Trigger::Start, Effect::Message(s("Hi")), Condition::Always),
        rule(
            Trigger::Start,
            Effect::Number(s("core.light"), 20),
            Condition::Always,
        ),
        rule(Trigger::Sleep, Effect::Message(s("no")), Condition::Always),
    ];
    let off = |_: &str| Ok(Some(Value::Bool(false)));
    let mut chain = Chain::default();
    let mut out = Vec::new();
    engine::run(
        &rules,
        |t| *t == Trigger::Start,
        &facts(&off, false),
        0,
        &mut chain,
        &mut out,
    )
    .unwrap();
    assert_eq!(
        out,
        [
            Act::Set(s(NIGHT), Value::Bool(true)),
            Act::Run(s("opds.sync")),
            Act::Announce(s("Hi")),
            Act::Set(s("core.light"), Value::Int(20)),
        ]
    );
    assert_eq!(chain.len(), 2);
    // A setting that has the value is not set again; a paused rule does nothing.
    let on = |_: &str| Ok(Some(Value::Bool(true)));
    let mut paused = rules.clone();
    paused[1].on = false;
    out.clear();
    engine::run(
        &paused,
        |t| *t == Trigger::Start,
        &facts(&on, false),
        0,
        &mut chain,
        &mut out,
    )
    .unwrap();
    assert!(!out.contains(&Act::Set(s(NIGHT), Value::Bool(true))));
    assert!(!out.contains(&Act::Run(s("opds.sync"))));
}

#[test]
fn conditions_hold_as_they_say() {
    let night_on = |id: &str| Ok((id == NIGHT).then_some(Value::Bool(true)));
    let f = facts(&night_on, true);
    assert!(engine::holds(&Condition::Always, &f).unwrap());
    assert!(engine::holds(&Condition::Reading, &f).unwrap());
    assert!(!engine::holds(&Condition::NotReading, &f).unwrap());
    assert!(engine::holds(&Condition::OnBattery, &f).unwrap());
    assert!(!engine::holds(&Condition::Charging, &f).unwrap());
    assert!(engine::holds(&Condition::SwitchOn(s(NIGHT)), &f).unwrap());
    assert!(!engine::holds(&Condition::SwitchOff(s(NIGHT)), &f).unwrap());
    assert!(engine::holds(&Condition::SwitchOff(s("core.other")), &f).unwrap());
    let between = Condition::Between {
        from: 22 * 60,
        to: 6 * 60,
    };
    assert!(engine::holds(&between, &f).unwrap());
    // No clock: a time of day does not hold.
    let mut unknown = facts(&night_on, true);
    unknown.minute = None;
    assert!(!engine::holds(&between, &unknown).unwrap());
    // A refused read is the host's refusal, not "off".
    let refused = |_: &str| Err(Error::Io);
    assert_eq!(
        engine::holds(&Condition::SwitchOn(s(NIGHT)), &facts(&refused, false)),
        Err(Error::Io)
    );
}

#[test]
fn rules_that_undo_each_other_are_counted_in_the_chain() {
    let mut chain = Chain::default();
    chain.caused("a.b", 1).unwrap();
    chain.caused("a.b", 3).unwrap();
    chain.caused("c.d", 2).unwrap();
    assert_eq!(chain.len(), 2);
    assert_eq!(chain.depth_of("a.b"), 3);
    assert_eq!(chain.depth_of("a.b"), 0, "heard of once");
    assert_eq!(chain.depth_of("never"), 0);
    assert_eq!(chain.depth_of("c.d"), 2);
    assert!(chain.is_empty());
    assert_eq!(MAX_CHAIN, 4);
    crate::fail_alloc::sweep(|| chain.caused("e.f", 1));
}

#[test]
fn the_battery_compares_with_how_it_was() {
    let mut battery = Battery::default();
    assert_eq!(
        battery.look(75, false),
        None,
        "the first look only remembers"
    );
    let rise = battery.look(81, true).unwrap();
    assert!(rise.sets_off(&Trigger::BatteryAtLeast(80)));
    assert!(rise.sets_off(&Trigger::Charging));
    assert!(!rise.sets_off(&Trigger::Unplugged));
    assert!(!rise.sets_off(&Trigger::BatteryBelow(80)));
    // Wavering around the mark: not again.
    assert!(
        !battery
            .look(79, true)
            .unwrap()
            .sets_off(&Trigger::BatteryAtLeast(80))
    );
    assert!(
        !battery
            .look(81, true)
            .unwrap()
            .sets_off(&Trigger::BatteryAtLeast(80))
    );
    let fall = battery.look(19, false).unwrap();
    assert!(fall.sets_off(&Trigger::BatteryBelow(20)));
    assert!(fall.sets_off(&Trigger::Unplugged));
    assert!(!fall.sets_off(&Trigger::Start));
    // Down clearly, then up again: once more.
    assert!(
        battery
            .look(82, false)
            .unwrap()
            .sets_off(&Trigger::BatteryAtLeast(80))
    );
}

#[test]
fn time_triggers_follow_the_clock_and_the_days() {
    let at = |m, days| Trigger::Time { minute: m, days };
    assert!(engine::time_due(&at(420, WEEKDAYS), 420, 1));
    assert!(!engine::time_due(&at(420, WEEKDAYS), 420, 0), "Sunday");
    assert!(!engine::time_due(&at(420, WEEKDAYS), 421, 1));
    assert!(engine::time_due(&at(420, WEEKENDS), 420, 6));
    assert!(
        !engine::time_due(&at(420, EVERY_DAY), 420, 7),
        "no such day"
    );
    assert!(!engine::time_due(&Trigger::Start, 0, 0));
    assert!(engine::every_due(&Trigger::Every(30), 60));
    assert!(!engine::every_due(&Trigger::Every(30), 59));
    assert!(!engine::every_due(&Trigger::Every(0), 0));
    assert!(engine::switch_due(
        &Trigger::SwitchOn(s(NIGHT)),
        NIGHT,
        true
    ));
    assert!(!engine::switch_due(
        &Trigger::SwitchOn(s(NIGHT)),
        NIGHT,
        false
    ));
    assert!(engine::switch_due(
        &Trigger::SwitchOff(s(NIGHT)),
        NIGHT,
        false
    ));
    assert!(!engine::switch_due(
        &Trigger::SwitchOff(s(NIGHT)),
        "x.y",
        false
    ));
    // 2026-11-07 (Saturday) 15:31 UTC; Berlin is an hour ahead.
    let unix = 1_794_065_460;
    assert_eq!(engine::local_minute(unix, 0), (15 * 60 + 31, 6));
    assert_eq!(engine::local_minute(unix, 3600), (16 * 60 + 31, 6));
    // Over midnight into Sunday, and before the epoch.
    assert_eq!(engine::local_minute(unix + 9 * 3600, 0).1, 0);
    assert_eq!(engine::local_minute(-1, 0), (23 * 60 + 59, 3));
    assert!(engine::local_minute(i64::MAX, i32::MAX).0 < 24 * 60);
    let _ = rule::MAX_FILE;
}
