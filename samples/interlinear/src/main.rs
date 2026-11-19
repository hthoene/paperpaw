//! Interlinear, a PaperPaw sample: a translation line under each sentence of
//! a book, for plugin interface 7.
//!
//! It subscribes to the reader's text stage (`reader.text`, see
//! `paperpaw_sdk::native::page_layers`). When a book opens the reader asks
//! whether the plugin adds to it: it does where its data folder holds the
//! book's side file (`alice-in-wonderland.epub` → `alice-in-wonderland.txt`),
//! which it reads into a small index once. Then the reader shows it each block
//! of the book before laying it out, once while the book is open, and the
//! plugin answers the translation of each sentence it finds: set under the
//! sentence in small italics, or after the paragraph. Page turns ask nothing.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use interlinear::{Index, Place};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    declare::{Declarations, DeclareError, Kind, SpecDecl, SubscribeDecl},
    events::{Event, Incoming, kinds},
    page_layers::{MAX_GLOSS, TEXT_STAGE, TextAnswer, TextAsk},
    registrations,
    reply::Reply,
    settings::{SettingValue, read_setting},
    storage::{self, Root},
};

/// The number the device hands back with the text stage's calls.
const TEXT: u32 = 1;
const ON: &str = "interlinear.on";
const AFTER: &str = "interlinear.after";
/// The bytes of the side file read at a time.
const PART: usize = 1024;

/// The book on show: its side file and that file's index.
struct Book {
    file: String,
    index: Index,
}

/// The plugin's state: the book it adds to.
struct Interlinear {
    book: Option<Book>,
}

/// What the plugin brings: two settings, and its subscription to the
/// reader's text stage.
fn declare(host: &Host<'_>) -> Result<(), DeclareError> {
    let mut d = Declarations::new(|batch| registrations::submit(host, batch).map_err(i32::from));
    let group = "core.group.reading-aids";
    let mut on = SpecDecl::new("on", Kind::Bool(true), group, "interlinear.setting.on");
    on.help = "interlinear.setting.on.help";
    d.spec(on)?;
    let mut after = SpecDecl::new(
        "after",
        Kind::Bool(false),
        group,
        "interlinear.setting.after",
    );
    after.help = "interlinear.setting.after.help";
    d.spec(after)?;
    d.subscribe(SubscribeDecl::new(TEXT_STAGE, TEXT))?;
    d.submit()
}

fn flag(host: &Host<'_>, key: &str, default: bool) -> bool {
    let mut scratch = [0u8; 32];
    match read_setting(host, key, &mut scratch) {
        Ok(SettingValue::Bool(value)) => value,
        _ => default,
    }
}

/// The index of the side file `file`; `None` where there is none.
fn read_index(host: &Host<'_>, file: &str) -> Option<Index> {
    let size = storage::size(host, Root::Data, file).ok()?;
    let mut index = Index::default();
    let mut part = [0u8; PART];
    let mut at = 0;
    while at < size {
        let n = storage::read_at(host, Root::Data, file, at, &mut part).ok()?;
        if n == 0 {
            break;
        }
        index.read(&part[..n]).ok()?;
        at += n as u64;
    }
    index.finish().ok()?;
    (!index.is_empty()).then_some(index)
}

impl Interlinear {
    /// The book at `path` opened: its side file read, and whether the plugin
    /// adds to it (a signature of the settings and the file).
    fn book(
        &mut self,
        host: &Host<'_>,
        path: &str,
        answer: &mut TextAnswer<'_>,
    ) -> Result<(), Error> {
        self.book = None;
        if !flag(host, ON, true) {
            return Ok(());
        }
        let Some(file) = interlinear::side_file(path) else {
            return Ok(());
        };
        let Some(index) = read_index(host, &file) else {
            return Ok(());
        };
        let after = flag(host, AFTER, false);
        let signature = (index.len() as u64) << 1 | u64::from(after) | 1 << 40;
        self.book = Some(Book { file, index });
        answer.uses(signature)
    }

    /// A block of the book: the translations of its sentences.
    fn block(&self, host: &Host<'_>, text: &str, answer: &mut TextAnswer<'_>) -> Result<(), Error> {
        let Some(book) = &self.book else {
            return Ok(());
        };
        let place = if flag(host, AFTER, false) {
            Place::After
        } else {
            Place::Under
        };
        // On the heap: a plugin's frame stays under 2,048 bytes.
        let mut read = alloc::vec::Vec::new();
        read.try_reserve_exact(MAX_GLOSS + 8)
            .map_err(|_| Error::NoMemory)?;
        read.resize(MAX_GLOSS + 8, 0);
        interlinear::answer(answer, text, place, |sentence| {
            let at = book.index.find(interlinear::key(sentence))?;
            let n =
                storage::read_at(host, Root::Data, &book.file, u64::from(at), &mut read).ok()?;
            let found = interlinear::translation(&read[..n]);
            (!found.is_empty()).then(|| String::from(found))
        })
    }
}

impl Plugin for Interlinear {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        declare(host).map_err(DeclareError::to_error)?;
        Ok(Self { book: None })
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        if token != TEXT {
            return Ok(());
        }
        let Some(Event::StageCall { payload, .. }) = incoming.decode_as::<kinds::StageCall>()?
        else {
            return Ok(());
        };
        let ask = TextAsk::decode(payload)?;
        reply.write_with(|out| {
            let mut answer = TextAnswer::new(out)?;
            // What does not fit the answer is left out; the rest is shown.
            let _ = match ask {
                TextAsk::Book { path, .. } => self.book(host, path, &mut answer),
                TextAsk::Block(block) => self.block(host, block.text, &mut answer),
                _ => Ok(()),
            };
            answer.finish()
        })
    }
}

// What the plugin calls of the device, as the record the packer puts into its
// file: made from `native/metadata.json` by `paperpaw-dev metadata`.
#[cfg(target_arch = "riscv32")]
const CALLS: &[u8] = include_bytes!("../native/metadata.bin");
#[cfg(target_arch = "xtensa")]
const CALLS: &[u8] = include_bytes!("../native/metadata.s3.bin");
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
const CALLS: &[u8] = include_bytes!("../native/metadata.host.bin");

// The plugin's entry, its heap and its panic handler.
paperpaw_sdk::native_plugin!(Interlinear, metadata = CALLS);
