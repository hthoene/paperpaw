//! The home screen's pages as `paperpaw.ini` keeps them, version 1
//! (`testdata/pages.ini.v1`): read by the home screen as written, and
//! what the current writer keeps after an edit read back the same, items
//! not touched written as they were.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::common::*;

const V1: &str = include_str!("../../../../plugins/home/testdata/pages.ini.v1");

fn page(app: &App, n: usize) -> String {
    app.kernel
        .settings()
        .text(&format!("home.page-{n}"))
        .to_string()
}

#[test]
fn version_1_pages_are_read_as_written() {
    for device in DEVICES {
        let mut app = App::new(device, Shared::memory(V1, &[]));
        let settings = app.kernel.settings();
        assert_eq!(settings.int("home.columns"), 4);
        assert_eq!(settings.int("home.rows"), 6);
        assert!(page(&app, 1).starts_with("tool:core.library 0/0 1x1"));
        // The tools where they were written: two side by side, one at
        // the right end; the book being read below the clock.
        let library = app.find("Library").unwrap();
        let settings = app.find("Settings").unwrap();
        let files = app.find("Plugin store").unwrap();
        let start = app.find("Start reading").unwrap();
        let cell = settings.x - library.x;
        assert!(cell > 0);
        // (The middles of the names, which differ in width.)
        assert!(((files.x - library.x) - 3 * cell).abs() < cell / 2);
        assert_eq!(library.y, files.y);
        assert!(start.y > library.y + cell, "{start:?} {library:?}");
        // The second page: every tool.
        app.run(actions::NEXT_PAGE);
        assert!(app.shows("Vocabulary"), "{}", app.described());
    }
}

#[test]
fn what_the_current_writer_keeps_is_read_back_the_same() {
    for device in DEVICES {
        let storage = Shared::memory(V1, &[]);
        let mut app = App::new(device, storage.clone());
        let written = page(&app, 1);
        // An edit: the file manager's tool moved one cell left and back.
        app.run(Id::new_static("home.edit-pages"));
        if device.input.touch {
            let files = app.find("Plugin store").unwrap();
            let library = app.find("Library").unwrap();
            let settings = app.find("Settings").unwrap();
            let cell = settings.x - library.x;
            app.drag(files, Point::new(files.x - cell, files.y));
            assert!(page(&app, 1).contains("tool:store.open 2/0 1x1"));
            let files = app.find("Plugin store").unwrap();
            app.drag(files, Point::new(files.x + cell, files.y));
        } else {
            // The third tool: the first press shows the focus.
            app.run(actions::RIGHT);
            app.repeat(actions::RIGHT, 2);
            for step in [actions::LEFT, actions::RIGHT] {
                app.run(actions::CONFIRM);
                if !app.parts().iter().any(|p| p.role == Role::Dialog) {
                    // Back on the page, the first press shows the focus.
                    app.run(actions::CONFIRM);
                }
                assert!(
                    app.parts().iter().any(|p| p.role == Role::Dialog),
                    "{}",
                    app.described()
                );
                // Move is the first option.
                app.run(actions::CONFIRM);
                let left = step == actions::LEFT;
                app.run(step);
                app.run(actions::CONFIRM);
                if left {
                    assert!(page(&app, 1).contains("tool:store.open 2/0 1x1"));
                }
            }
        }
        app.run(actions::BACK);
        let kept = page(&app, 1);
        // Every item as it was, in the order written.
        assert_eq!(kept, written, "{}", storage.config());
        let mut app = app.restart();
        assert_eq!(page(&app, 1), written);
        assert!(app.shows("Library"));
    }
}
