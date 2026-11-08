//! The screen lock's native declarations (slice 1): its page of settings (the
//! kind of lock, whether waking up asks for it, the touch lock's switch). The
//! page's icon and the first-run step are declarable but the host refuses them
//! until it takes icons and setup steps; the lock screen, the actions, the menu
//! entries, the input filter and the status item follow with their host pieces
//! (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, GroupDecl, Kind, SpecDecl};

/// The page's group, relative to the plugin; in full `lock.group`.
const GROUP: &str = "group";
const GROUP_FULL: &str = "lock.group";

/// The kinds of lock as the setting stores them.
pub const KIND_VALUES: [&str; 4] = [
    "lock.kind.off",
    "lock.kind.pin",
    "lock.kind.pattern",
    "lock.kind.password",
];

/// The page of the lock's settings.
pub fn settings<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    let mut group = GroupDecl::new(GROUP, "lock.plugin.name");
    group.order = 92;
    d.group(group)?;
    let mut kind = SpecDecl::new(
        "kind",
        Kind::Choice {
            default: KIND_VALUES[0],
            choices: &KIND_VALUES,
        },
        GROUP_FULL,
        "lock.setting.kind",
    );
    kind.help = "lock.setting.kind.help";
    kind.guarded = true;
    d.spec(kind)?;
    let mut on_wake = SpecDecl::new(
        "on-wake",
        Kind::Bool(true),
        GROUP_FULL,
        "lock.setting.on-wake",
    );
    on_wake.help = "lock.setting.on-wake.help";
    on_wake.guarded = true;
    d.spec(on_wake)?;
    let mut touch = SpecDecl::new("touch", Kind::Bool(false), GROUP_FULL, "lock.touch");
    touch.help = "lock.touch.help";
    d.spec(touch)?;
    d.submit()?;
    Ok(())
}
