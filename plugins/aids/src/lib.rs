//! Reading aids, as text filters (see `paperpaw_sdk::filters`), set under
//! Settings › Reading › Reading aids:
//!
//! - *Bold word beginnings*: the first half of every word in bold, which
//!   some readers follow more easily ("bionic reading").
//! - *Replace words*: pairs `from = to` separated by `;`, e.g. for old
//!   spellings or names; whole words only, as written.
//!
//! - *Pictures*: shown as the book has them, a short line in their place
//!   (the picture's description), or left out.
//!
//! All are off at first and apply to every book.

#![no_std]

extern crate alloc;

mod native_entry;
#[cfg(feature = "linked")]
pub use native_entry::NATIVE_ENTRY;

/// The settings the plugin declares, made by its build script from the same
/// declarations as a table.
#[cfg(feature = "linked")]
mod native_settings {
    use paperpaw_sdk::{Id, SettingSpec};
    include!(concat!(env!("OUT_DIR"), "/aids_settings.rs"));
}

/// The settings the plugin declares as a table kept in the program (as Beta 3
/// kept them), for a linked build to use in place of a copy on the heap of
/// each (`Kernel::register_linked`). Only a linked plugin has it.
#[cfg(feature = "linked")]
pub static NATIVE_SETTINGS: &[paperpaw_sdk::SettingSpec] = &native_settings::SETTINGS;
/// The interface-7 plugin.
pub mod native_plugin;
pub mod pictures;

pub mod reader_filter;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::groups;
use paperpaw_sdk::document::{Block, BlockKind, Run, TextStyle};
use paperpaw_sdk::filters::{FilterContext, TextFilterSpec};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::{Env, Id, Manifest, Plugin, RegisterError, Registrar, SettingSpec};

pub const PLUGIN: Id = Id::new_static("aids");
/// Setting: the first half of every word in bold.
pub const BOLD_BEGINNINGS: Setting<bool> = Setting::new("aids.bold-beginnings");
/// Setting: words to replace, `from = to; …`.
pub const REPLACE: Setting<Text> = Setting::new("aids.replace");
/// Setting: the pictures of books shown, as a caption, or left out
/// (one of [`pictures::CHOICES`]).
pub const PICTURES: Setting<Text> = Setting::new("aids.pictures");

/// The pairs of a replacement list: `from = to` separated by `;`.
fn pairs(list: &str) -> Vec<(String, String)> {
    list.split(';')
        .filter_map(|pair| {
            let (from, to) = pair.split_once('=')?;
            let from = from.trim();
            (!from.is_empty()).then(|| (from.to_string(), to.trim().to_string()))
        })
        .collect()
}

/// Whether the character before `at` and the one from `end` are no part
/// of a word.
fn whole_word(text: &str, at: usize, end: usize) -> bool {
    let before = text[..at].chars().next_back();
    let after = text[end..].chars().next();
    !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
}

/// Replaces the pairs' words in `block`, last first so earlier places
/// stay where they are.
fn replace(block: &mut Block, pairs: &[(String, String)]) {
    for (from, to) in pairs {
        let mut found: Vec<usize> = block
            .text
            .match_indices(from.as_str())
            .map(|(at, _)| at)
            .filter(|&at| whole_word(&block.text, at, at + from.len()))
            .collect();
        found.reverse();
        for at in found {
            block.replace(at..at + from.len(), to);
        }
    }
}

/// Bold runs for the first half of every word, where no other style is.
fn bold_beginnings(block: &mut Block) {
    if !matches!(block.kind, BlockKind::Paragraph) {
        return;
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut word: Option<(usize, usize)> = None; // start, letters
    let end_word = |start: usize, letters: usize, text: &str, runs: &mut Vec<Run>| {
        if letters < 2 {
            return;
        }
        let bold = letters.div_ceil(2);
        let end = text[start..]
            .char_indices()
            .nth(bold)
            .map_or(text.len(), |(i, _)| start + i);
        runs.push(Run::new(start as u32, end as u32, TextStyle::BOLD));
    };
    for (i, c) in block.text.char_indices() {
        match (&mut word, c.is_alphabetic()) {
            (Some((_, letters)), true) => *letters += 1,
            (None, true) => word = Some((i, 1)),
            (Some((start, letters)), false) => {
                end_word(*start, *letters, &block.text, &mut runs);
                word = None;
            }
            (None, false) => {}
        }
    }
    if let Some((start, letters)) = word {
        end_word(start, letters, &block.text, &mut runs);
    }
    // Not where the book styles the text itself.
    runs.retain(|run| {
        !block
            .runs
            .iter()
            .any(|r| r.start < run.end && run.start < r.end)
    });
    block.runs.extend(runs);
    paperpaw_sdk::sort::sort_by_key(&mut block.runs, |run| run.start);
}

/// The reading aids plugin.
#[derive(Debug, Default)]
pub struct Aids;

/// Bold word beginnings, words replaced, pictures.
static SETTINGS: [SettingSpec; 3] = [
    SettingSpec::switch(
        BOLD_BEGINNINGS.static_str(),
        "aids.setting.bold-beginnings",
        false,
        groups::READING_AIDS,
    )
    .description("aids.setting.bold-beginnings.help"),
    SettingSpec::text(
        REPLACE.static_str(),
        "aids.setting.replace",
        "",
        groups::READING_AIDS,
    )
    .advanced()
    .kept_on_reset()
    .description("aids.setting.replace.help"),
    SettingSpec::choice(
        PICTURES.static_str(),
        "aids.setting.pictures",
        &pictures::CHOICES,
        pictures::SHOW,
        groups::READING_AIDS,
    )
    .description("aids.setting.pictures.help"),
];

/// How it works, by language (see `Manifest::how`).
static HOW: [(&str, &str); 2] = [
    ("en", include_str!("../how/en.txt")),
    ("de", include_str!("../how/de.txt")),
];

impl Plugin for Aids {
    fn manifest(&self) -> Manifest {
        Manifest::new(PLUGIN, "Reading aids", paperpaw_sdk::package_version!())
            .description("Bold word beginnings, replaced words, pictures: text as you want it.")
            .how(&HOW)
            .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_settings(&SETTINGS)?;
        r.add_text_filter(TextFilterSpec::new(
            Id::new_static("aids.bold"),
            "aids.setting.bold-beginnings",
            Box::new(|env: &Env<'_>, block: &mut Block| {
                if env.settings().read(&BOLD_BEGINNINGS) {
                    bold_beginnings(block);
                }
            }),
            Box::new(|env: &Env<'_>| u64::from(env.settings().read(&BOLD_BEGINNINGS))),
        ))?;
        r.add_text_filter(TextFilterSpec::new(
            Id::new_static("aids.replace"),
            "aids.setting.replace",
            Box::new(|env: &Env<'_>, block: &mut Block| {
                let list = pairs(env.settings().read(&REPLACE));
                if !list.is_empty() {
                    replace(block, &list);
                }
            }),
            Box::new(|env: &Env<'_>| {
                let list = env.settings().read(&REPLACE);
                let list = list.trim();
                if list.is_empty() {
                    0
                } else {
                    paperpaw_sdk::digest::fnv1a64(list)
                }
            }),
        ))?;
        r.add_text_filter(TextFilterSpec::block(
            Id::new_static("aids.pictures"),
            "aids.setting.pictures",
            Box::new(|env: &Env<'_>, cx: &FilterContext<'_>, block: &mut Block| {
                let choice = env.settings().read(&PICTURES);
                pictures::apply(choice, cx.path, env.i18n(), block)
            }),
            Box::new(|env: &Env<'_>| pictures::signature(env.settings().read(&PICTURES))),
        ))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/en.ini")))?;
        r.add_translations(Cow::Borrowed(include_str!("../i18n/de.ini")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_i18n::{Catalog, check};
    use paperpaw_sdk::document::{Align, OffsetMap};

    fn block(text: &str) -> Block {
        Block::new(BlockKind::Paragraph, 0, text.len() as u32, text.to_string())
            .map(OffsetMap::IDENTITY)
            .align(Align::default())
    }

    #[test]
    fn words_are_replaced_whole() {
        let mut b = block("Mr. Smith and Mr. Smithers met.");
        replace(&mut b, &pairs("Smith = Jones; Mr. = Mister"));
        assert_eq!(b.text, "Mister Jones and Mister Smithers met.");
        assert_eq!(
            pairs(" a = b ;; =x; c="),
            [
                (String::from("a"), String::from("b")),
                (String::from("c"), String::new())
            ]
        );
    }

    #[test]
    fn beginnings_are_bold() {
        let mut b = block("Read it, Alice!");
        bold_beginnings(&mut b);
        let bold: Vec<&str> = b
            .runs
            .iter()
            .map(|r| &b.text[r.start as usize..r.end as usize])
            .collect();
        assert_eq!(bold, ["Re", "i", "Ali"]);
    }

    #[test]
    fn translations_are_complete() {
        let en = Catalog::parse(include_str!("../i18n/en.ini")).unwrap();
        let de = Catalog::parse(include_str!("../i18n/de.ini")).unwrap();
        let issues = check::compare(&en, &de);
        assert!(issues.is_empty(), "{issues:?}");
    }
}

#[cfg(test)]
mod filters_tests;
