//! *Clean up storage*: what is kept for books no longer found goes,
//! caches by themselves, the reader's own data only when asked.

use crate::kit::library::BookDataKind;

use super::*;

/// What the kernel keeps for books, and cleaning up what is left of books
/// no longer found: caches by themselves, the reader's data when asked.
pub(super) fn register_clean_up(r: &mut Staging) -> Result<(), RegisterError> {
    // The notes of books are the kernel's, and the details (with the
    // day a book came and plugins' fields, which are not read again).
    r.add_book_data(paperpaw_sdk::data_path!("annotations"), "")?;
    r.add_book_data(paperpaw_sdk::positions::DIR, "")?;
    r.add_book_data(crate::kit::readers::DIR, "")?;
    r.add_book_data(alloc::format!("{}/details", library::DIR), "")?;
    r.add_book_cache(alloc::format!("{}/thumbs", library::DIR), "")?;
    // With the device's settings.
    static ENTRY: [crate::kit::menus::MenuEntry; 1] = [crate::kit::menus::MenuEntry::new(
        groups::SYSTEM,
        actions::CLEAN_UP,
    )];
    static ICON: [(paperpaw_sdk::Id, &str); 1] = [(actions::CLEAN_UP, "eraser")];
    r.add_menu_entries(&ENTRY)?;
    r.add_icons(&ICON)?;
    r.on(CLEAN_UP_ANSWERED, |k, p| {
        k.with_event(&CLEAN_UP_ANSWERED, p, |env, answer, out| {
            if answer.yes {
                let text = clean_up(env, out, BookDataKind::UserData);
                out.push(Ask::Notify(text));
            }
        });
    })?;
    // And by itself now and then, silently: caches only.
    r.add_work(crate::work::WorkSpec::new(
        CLEAN_UP_WORK,
        "core.work.clean-up",
        |k, _| {
            k.with_env(|env, out| {
                // What it found is its note, no notice.
                let text = clean_up(env, out, BookDataKind::Cache);
                out.push(Ask::Notify(text));
            });
            paperpaw_sdk::work::Step::Done
        },
    ))?;
    r.on(events::STARTED, |k, p| {
        k.with_event(&events::STARTED, p, |env, (), out| {
            declare_clean_up(env, out)
        });
    })?;
    Ok(())
}

/// Cleans up the caches of books not found, then asks whether the
/// reader's data of those books goes too, saying of how many: a book
/// renamed on a computer, or on a card not inserted, is not found either.
pub(super) fn ask_clean_up(env: &crate::Place<'_>, out: &mut crate::Asks) {
    let text = clean_up(env, out, BookDataKind::Cache);
    let user = BookDataKind::UserData;
    match library::unused_books(env.storage(), env.book_data(), library::folder(env), user) {
        Ok(count) if count > 0 => {
            let question = env
                .i18n()
                .plural("core.clean-up.question", i64::from(count), &[]);
            out.push(
                crate::kit::ask::confirm(question, &CLEAN_UP_ANSWERED)
                    .text(env.i18n().tr("core.clean-up.text"))
                    .yes(env.i18n().tr("core.clean-up.yes"))
                    .no(env.i18n().tr("core.clean-up.no"))
                    .command(),
            );
        }
        _ => out.push(Ask::Notify(text)),
    }
}

/// Removes what is kept of `kind` for files no longer on the storage: of
/// any format, by any spelling; nothing where a folder could not be read.
/// Notes the time and declares the next time it is done by itself; what
/// it did, to say.
fn clean_up(env: &crate::Place<'_>, out: &mut crate::Asks, kind: BookDataKind) -> String {
    let library = library::folder(env);
    let text = match library::clean_up_unused(env.storage(), env.book_data(), library, kind) {
        Ok(removed) => env.i18n().format(
            "core.cleaned-up",
            &[("count", paperpaw_sdk::Arg::Int(i64::from(removed)))],
        ),
        Err(_) => String::from(env.i18n().tr("core.clean-up-failed")),
    };
    if let Some(now) = crate::kit::time::unix_seconds(env) {
        let _ = env
            .storage()
            .write(CLEANED_UP, alloc::format!("{now}\n").as_bytes());
        declare_clean_up(env, out);
    }
    text
}
