//! Buttons alone on an emulated X4: the battery page is reached from the
//! tools and closes; a plugin made to fail has a screen nothing leads to
//! and one that keeps back and the focus to itself.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_core::KernelBuilder;
use paperpaw_core::{Place, Turn};
use paperpaw_dev::cli::Context;
use paperpaw_dev::reach::{self, Limits};
use paperpaw_dev::report::{Finding, Level, Report};
use paperpaw_dev::target::Target;
use paperpaw_dev::variant::Variant;
use paperpaw_platform::Rect;
use paperpaw_sdk::builtin::menus;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::{
    ActionSpec, Canvas, Command, Commands, Handled, Id, Manifest, MenuEntry, Params, Version,
};

const OPEN: Id = Id::new_static("trapped.open");
const STUCK: Id = Id::new_static("trapped.stuck");
const HIDDEN: Id = Id::new_static("trapped.hidden");

/// A tool whose screen keeps every button to itself, and a screen
/// nothing opens.
fn trapped(builder: KernelBuilder) -> KernelBuilder {
    let manifest = Manifest::new(Id::new_static("trapped"), "Trapped", Version::new(0, 1, 0))
        .description("Fails the reach check on purpose.")
        .uses(paperpaw_sdk::Uses::NOTHING);
    builder.items(manifest, |items| {
        items.add_fake(paperpaw_core::testing::FakeSpec::new(
            STUCK,
            "trapped.stuck",
            Box::new(|_, _| Box::new(Stuck)),
        ))?;
        items.add_fake(paperpaw_core::testing::FakeSpec::new(
            HIDDEN,
            "trapped.hidden",
            Box::new(|_, _| Box::new(Hidden)),
        ))?;
        items.add_action(ActionSpec::new(OPEN, "trapped.open").handler(
            |_: &paperpaw_sdk::Env<'_>, _, out: &mut Commands| {
                out.push(Command::OpenScreen(STUCK, Params::new()));
            },
        ))?;
        items.add_menu_entry(MenuEntry::new(menus::TOOLS, OPEN))
    })
}

fn rows(canvas: &mut Canvas<'_>, labels: &[&str], focused: Option<usize>) {
    for (i, label) in labels.iter().enumerate() {
        let area = Rect::new(0, 100 + 60 * i as u16, 480, 60);
        canvas
            .describe(Role::Row, label, area)
            .focused(focused == Some(i));
    }
}

/// Three rows, the focus on the first for good, every button taken.
struct Stuck;

impl paperpaw_core::testing::Fake for Stuck {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        rows(canvas, &["One", "Two", "Three"], Some(0));
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::Yes
    }
}

/// A row; back closes it, but nothing opens it.
struct Hidden;

impl paperpaw_core::testing::Fake for Hidden {
    fn draw(&mut self, canvas: &mut Canvas<'_>, _: &Place<'_>) {
        rows(canvas, &["Alone"], Some(0));
    }

    fn on_action(&mut self, _: &Id, _: &mut Turn<'_, '_>) -> Handled {
        Handled::No
    }
}

fn add(builder: KernelBuilder) -> KernelBuilder {
    trapped(builder)
}

fn found<'a>(report: &'a Report, check: &str, place: &str) -> Option<&'a Finding> {
    report
        .findings
        .iter()
        .find(|f| f.check == check && f.place == place)
}

#[test]
fn screens_are_reached_and_closed_with_buttons() {
    let out = std::env::temp_dir().join(format!("paperpaw-dev-reach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let context = Context::new(
        Target {
            plugins: vec!["battery".into(), "trapped".into()],
            add: Some(add),
            ..Target::default()
        },
        out.clone(),
    );
    let limits = Limits {
        views: 12,
        ..Limits::default()
    };
    let mut report = Report::default();
    reach::check(&context, &Variant::x4(), &limits, &mut report).unwrap();
    let text = report.text(Level::Info);

    // The battery page: from the tools, closed with back, nothing to
    // move the focus through.
    assert!(
        !report.findings.iter().any(|f| f.place == "battery.screen"),
        "{text}"
    );

    // Nothing leads to the hidden screen.
    let unreachable = found(&report, "reach.unreachable", "trapped.hidden").expect(&text);
    assert_eq!(unreachable.level, Level::Warning);
    assert_eq!(unreachable.plugin.as_deref(), Some("trapped"));
    assert!(out.join(unreachable.picture.as_ref().unwrap()).is_file());

    // The stuck screen is reached from the tools, but keeps back and the
    // focus.
    assert!(
        found(&report, "reach.unreachable", "trapped.stuck").is_none(),
        "{text}"
    );
    assert!(
        found(&report, "reach.no-close", "trapped.stuck").is_some(),
        "{text}"
    );
    let trap = found(&report, "reach.focus-trap", "trapped.stuck").expect(&text);
    assert!(trap.message.contains("Row \"One\""), "{}", trap.message);
    let skips = found(&report, "reach.focus-skips", "trapped.stuck").expect(&text);
    assert!(skips.message.contains("Row \"Two\""), "{}", skips.message);
    assert!(skips.message.contains("Row \"Three\""), "{}", skips.message);
    let _ = std::fs::remove_dir_all(&out);
}
