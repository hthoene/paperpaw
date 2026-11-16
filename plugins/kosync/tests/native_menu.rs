//! The native plugin contributes Sync progress only while installed and enabled.
#![cfg(feature = "linked")]
#![allow(clippy::unwrap_used, reason = "test assertions")]
mod host;
use host::World;
use paperpaw_sdk::{
    Instant, Params,
    builtin::{params, slots},
};

#[test]
fn sync_progress_is_in_reader_more_only_when_kosync_is_installed_and_enabled() {
    for (installed, enabled) in [(false, false), (true, false), (true, true)] {
        let mut world = World::menu(installed, enabled);
        world.kernel().open_slot(
            Instant(100),
            slots::MENU,
            Params::new().with(params::MENU, "core.reader-more"),
        );
        world.pump();
        world.kernel().render(&mut |_, _| {});
        world.pump();
        let labels: Vec<_> = world
            .kernel()
            .describe()
            .into_iter()
            .map(|p| p.label)
            .collect();
        let present = labels.iter().any(|label| label.contains("Sync progress"));
        assert_eq!(
            present,
            installed && enabled,
            "installed={installed}, enabled={enabled}, labels={labels:?}"
        );
        world.assert_clean();
    }
}
