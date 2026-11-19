//! The interface-7 Gloss: the size setting, and the answer to the reader's
//! text stage for each paragraph of a book that has a word list.
use crate::glossary::{self, Glossary};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host, Plugin,
    events::{Event, Incoming, kinds},
    page_layers::{TEXT_STAGE, TextAnswer, TextAsk},
    registrations,
    reply::Reply,
    settings::{SettingValue, read_setting},
    storage::{self, Root},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/gloss_declarations.rs"));
}

const SIZE: &str = "gloss.size";
/// The bytes of the list read at a time.
const PART: usize = 1024;

/// The size of the small text in percent of the text, for a size choice.
fn percent(choice: &str) -> u16 {
    match choice {
        "gloss.size.small" => 40,
        "gloss.size.large" => 70,
        _ => 55,
    }
}

/// The book on show: its word list and the size of the small text.
#[derive(Debug)]
struct Book {
    glossary: Glossary,
    size: u16,
}

/// The interface-7 Gloss.
#[derive(Debug, Default)]
pub struct NativeGloss {
    book: Option<Book>,
}

/// The word list in the data file `file`; `None` where there is none or it
/// holds no word.
fn read_list(host: &Host<'_>, file: &str) -> Option<Glossary> {
    let size = storage::size(host, Root::Data, file).ok()?;
    let mut part = Vec::new();
    part.try_reserve_exact(PART).ok()?;
    part.resize(PART, 0);
    let mut glossary = Glossary::default();
    let mut at = 0;
    while at < size {
        let n = storage::read_at(host, Root::Data, file, at, &mut part).ok()?;
        if n == 0 {
            break;
        }
        glossary.read(&part[..n]).ok()?;
        at += n as u64;
    }
    glossary.finish().ok()?;
    (!glossary.is_empty()).then_some(glossary)
}

impl NativeGloss {
    /// The book at `path` opened: its list read once, and whether the plugin
    /// adds to it (a signature of the size and the list).
    fn open(
        &mut self,
        host: &Host<'_>,
        path: &str,
        language: &str,
        answer: &mut TextAnswer<'_>,
    ) -> Result<(), Error> {
        self.book = None;
        let files: [Option<String>; 2] =
            [glossary::book_file(path), glossary::language_file(language)];
        let Some(glossary) = files
            .iter()
            .flatten()
            .find_map(|file| read_list(host, file))
        else {
            return Ok(());
        };
        let mut scratch = [0u8; 64];
        let choice = match read_setting(host, SIZE, &mut scratch) {
            Ok(SettingValue::Text(text)) => text,
            _ => "",
        };
        let size = percent(choice);
        let signature = u64::from(glossary.hash()) << 32 | u64::from(size) << 1 | 1;
        self.book = Some(Book { glossary, size });
        answer.uses(signature)
    }
}

impl Plugin for NativeGloss {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS {
            registrations::submit(host, batch)?;
        }
        // A host that does not take the subscription leaves the setting as it
        // is: the reader has nothing to ask for.
        for batch in image::STAGES {
            match registrations::submit(host, batch) {
                Ok(()) | Err(Error::Unsupported) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        _token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(Event::StageCall { stage, payload, .. }) =
            incoming.decode_as::<kinds::StageCall>()?
        else {
            return Ok(());
        };
        if stage != TEXT_STAGE {
            return Ok(());
        }
        let ask = TextAsk::decode(payload)?;
        reply.write_with(|out| {
            let mut answer = TextAnswer::new(out)?;
            // What does not fit the answer is left out; the rest is shown.
            let _ = match ask {
                TextAsk::Book { path, language } => self.open(host, path, language, &mut answer),
                TextAsk::Block(block) => match &self.book {
                    Some(book) => {
                        glossary::answer(&mut answer, block.text, &book.glossary, book.size)
                    }
                    None => Ok(()),
                },
                _ => Ok(()),
            };
            answer.finish()
        })
    }
}
