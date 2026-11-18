//! Firmware updates, on devices whose firmware PaperPaw is (the X4): a page in
//! the settings finds an update file in the folder `updates` on the card, says
//! what it is and who signed it, and has the device install it. An update found
//! at start is offered once; after the restart the plugin says how it went and
//! removes the file. The page also asks the update server for a newer firmware
//! and downloads it into `updates`, and then the code plugins built for it. An
//! update whose plugins have no builds for it yet waits for them. Where the
//! reader chose it, the device looks for updates by itself about once a week, at
//! a cheap moment. An interface-7 native plugin ([`native_plugin`]); what is not
//! native yet is named in `PORTING-7.md`.
//!
//! Updates signed by PaperPaw install at once; one's own builds, signed with
//! one's own key, once the reader accepted that key, shown by its fingerprint,
//! at each install. An older version installs only when the reader chooses it,
//! told it is older.

#![no_std]

extern crate alloc;

#[cfg(test)]
extern crate std;
/// Test-only: refuses chosen allocations to prove the fallible paths.
#[cfg(test)]
#[path = "../../test-support/fail_alloc.rs"]
mod fail_alloc;
#[cfg(test)]
#[global_allocator]
static ALLOC: fail_alloc::Failing = fail_alloc::Failing;

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the declaration test also use the builders"
)]
mod declarations;

mod native_card;
mod native_check;
mod native_entry;
mod native_leaves;
mod native_online;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod native_update;
mod native_waiting;

#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

#[cfg(test)]
mod tests {
    use paperpaw_i18n::{Catalog, check};

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }

    /// Every text the native page names is in both languages.
    #[test]
    fn the_native_page_names_only_texts_that_exist() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        // Ids of the page, the actions, the settings and the work, which are no texts.
        let ids = [
            "update.open",
            "update.check",
            "update.page",
            "update.server",
            "update.look",
            "update.betas",
            "update.look-due",
            "update.check-due",
        ];
        let mut seen = 0;
        for source in [
            include_str!("native_plugin.rs"),
            include_str!("native_plugin/online.rs"),
            include_str!("native_plugin/fetch.rs"),
            include_str!("native_plugin/waiting.rs"),
            include_str!("native_plugin/leaves.rs"),
            include_str!("native_card.rs"),
            include_str!("native_online.rs"),
        ] {
            for piece in source.split("\"update.").skip(1) {
                let Some((name, _)) = piece.split_once('"') else {
                    continue;
                };
                let key = alloc::format!("update.{name}");
                if ids.contains(&key.as_str()) || key.ends_with('.') || key.contains(' ') {
                    continue;
                }
                seen += 1;
                // A message with a count is kept as its `.one` and `.other`.
                let one = alloc::format!("{key}.one");
                assert!(
                    en.get(&key).is_some() || en.get(&one).is_some(),
                    "{key} is not in en.ini"
                );
                assert!(
                    de.get(&key).is_some() || de.get(&one).is_some(),
                    "{key} is not in de.ini"
                );
            }
        }
        assert!(seen > 20, "{seen}");
    }
}

/// The settings the plugin declares, made by its build script from the same
/// declarations as tables.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/update_settings.rs"));
}

/// The settings the plugin declares as a table kept in the program (as Beta 3
/// kept them), for a linked build to use in place of a copy on the heap of
/// each (`Kernel::register_linked`). Only a linked plugin has it.
#[cfg(feature = "linked")]
pub static NATIVE_SETTINGS: &[paperpaw_sdk::SettingSpec] = &native_settings::SETTINGS;

/// The setting groups the plugin declares, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_GROUPS: &[paperpaw_sdk::SettingGroupSpec] = &native_settings::GROUPS;

/// The ids the plugin's declarations name, as a table kept in the program
/// like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_IDS: &[paperpaw_sdk::Id] = &native_settings::IDS;

/// The id and title of each screen and action the plugin declares, as a table
/// kept in the program like [`NATIVE_SETTINGS`].
#[cfg(feature = "linked")]
pub static NATIVE_TITLES: &[paperpaw_sdk::TitleEntry] = &native_settings::TITLES;
