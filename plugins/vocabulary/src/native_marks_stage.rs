//! The host's part of the marks in books: what the stage `reader.decorations@1`
//! asks of the plugin. A page's runs are answered with the marks of the words of
//! the list (the look from the role's settings), a tap on a mark opens the
//! lookup of its word. The rest is in `native_marks`.
use crate::{
    files::DataFolder,
    native_marks::{self, Entries, Look, ROLE, Settings, primary},
    stages_decorations::reader_decorations_v1::{
        DecorateLook, DecorateMarks, DecorateTaken, DecorateTapped, Record, input_decode,
        reply_encode,
    },
};
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    reply::Reply,
    settings::{SettingValue, read_setting},
    storage::DataStorage,
    ui::{Open, Param, Ui},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// The setting that switches the marks on.
const MARK: &str = "vocabulary.mark";

/// Answers one call of the stage: marks for a page, or a tap taken.
///
/// # Errors
/// `Invalid` for a call that is not the stage's, the host's refusals,
/// `NoMemory`.
pub(crate) fn decorate(
    host: &Host<'_>,
    ui: &mut Option<Ui>,
    payload: &[u8],
    reply: &mut Reply<'_>,
) -> Result<(), Error> {
    let mut records = input_decode(payload).map_err(|_| Error::Invalid)?;
    match records.next() {
        Some(Ok(Record::DecoratePage(page))) => {
            if !flag(host, MARK)? {
                return Ok(());
            }
            let storage = DataStorage::new(host);
            let entries = Entries::read(&DataFolder(&storage), primary(page.language))?;
            if entries.is_empty() {
                return Ok(());
            }
            let mut ranges = Vec::new();
            for (number, record) in records.enumerate() {
                let Ok(Record::DecorateRun(run)) = record else {
                    return Err(Error::Invalid);
                };
                let number = u16::try_from(number).map_err(|_| Error::Invalid)?;
                if !entries.find(run.text, number, &mut ranges)? {
                    break;
                }
            }
            if ranges.is_empty() {
                return Ok(());
            }
            let look = look(host)?;
            answer(reply, &look, &ranges)
        }
        Some(Ok(Record::DecorateTapped(tapped))) => taken(host, ui, &tapped, reply),
        _ => Err(Error::Invalid),
    }
}

/// The reply: the look, then the marks that use it.
fn answer(reply: &mut Reply<'_>, look: &Look, ranges: &[u8]) -> Result<(), Error> {
    let rows = [
        Record::DecorateLook(DecorateLook {
            look: 0,
            role: ROLE,
            background: look.background,
            ink: look.ink,
            line: look.line,
            line_tone: look.line_tone,
            thickness: look.thickness,
            flags: look.flags,
        }),
        Record::DecorateMarks(DecorateMarks::new(0, ranges)),
    ];
    reply.write_with(|out| reply_encode(&rows, out).map_err(|_| Error::NoSpace))
}

/// A tap on a mark of the role: its word is looked up, in the language it was
/// kept in; the tap is taken. A mark of another role is not ours.
fn taken(
    host: &Host<'_>,
    ui: &mut Option<Ui>,
    tapped: &DecorateTapped<'_>,
    reply: &mut Reply<'_>,
) -> Result<(), Error> {
    let word = native_marks::tapped_word(tapped.text);
    if tapped.role != ROLE || word.is_empty() {
        return Ok(());
    }
    let storage = DataStorage::new(host);
    let language = native_marks::language_of(&DataFolder(&storage), word)?;
    if ui.is_none() {
        *ui = Some(Ui::open(host)?);
    }
    let ui = ui.as_ref().ok_or(Error::State)?;
    let params = [
        Param::new("word", word)?,
        Param::new("language", &language)?,
    ];
    let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
    let handover = ui.handover(host, &params, &mut input, &mut out)?;
    CommandSender::new(host, &mut [0; 256]).open(Open::slot("core.lookup")?.handover(handover))?;
    reply.write_with(|out| {
        reply_encode(&[Record::DecorateTaken(DecorateTaken::new(1))], out)
            .map_err(|_| Error::NoSpace)
    })
}

/// A switch, off where it is not set.
fn flag(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut scratch = [0u8; 32];
    Ok(matches!(
        read_setting(host, key, &mut scratch)?,
        SettingValue::Bool(true)
    ))
}

/// The look of the role from its settings; a setting that is not set, or is
/// something else, is Beta 3's default.
fn look(host: &Host<'_>) -> Result<Look, Error> {
    let mut scratch = [[0u8; 64]; 4];
    let mut kept = [None::<&str>; 4];
    let [background, ink, line, tone] = &mut scratch;
    for (at, (key, room)) in [
        ("vocabulary.learning.background", background),
        ("vocabulary.learning.ink", ink),
        ("vocabulary.learning.line", line),
        ("vocabulary.learning.line-tone", tone),
    ]
    .into_iter()
    .enumerate()
    {
        if let SettingValue::Text(text) = read_setting(host, key, room)? {
            kept[at] = Some(text);
        }
    }
    let mut numbers = [0u8; 16];
    let thickness = match read_setting(host, "vocabulary.learning.thickness", &mut numbers)? {
        SettingValue::Int(value) => value,
        _ => 4,
    };
    let frame = flag(host, "vocabulary.learning.frame")?;
    let struck = flag(host, "vocabulary.learning.struck")?;
    let default = Settings::default();
    Ok(Look::from_settings(&Settings {
        background: kept[0].unwrap_or(default.background),
        ink: kept[1].unwrap_or(default.ink),
        line: kept[2].unwrap_or(default.line),
        line_tone: kept[3].unwrap_or(default.line_tone),
        thickness,
        frame,
        struck,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stages_decorations::reader_decorations_v1::{REPLY_ROWS, reply_decode};

    #[test]
    fn a_reply_of_the_most_marks_is_a_look_and_one_row_of_ranges_within_the_stages_limits() {
        let ranges = [0u8; native_marks::MAX_RANGES];
        let look = Look::default();
        let rows = [
            Record::DecorateLook(DecorateLook {
                look: 0,
                role: ROLE,
                background: look.background,
                ink: look.ink,
                line: look.line,
                line_tone: look.line_tone,
                thickness: look.thickness,
                flags: look.flags,
            }),
            Record::DecorateMarks(DecorateMarks::new(0, &ranges)),
        ];
        let mut out = [0u8; 3840];
        let used = reply_encode(&rows, &mut out).unwrap();
        assert!(used <= 3840 && rows.len() <= REPLY_ROWS);
        let back: Vec<_> = reply_decode(&out[..used]).unwrap().collect();
        assert_eq!(back.len(), 2);
        assert!(matches!(
            back[1],
            Ok(Record::DecorateMarks(marks)) if marks.ranges.len() == native_marks::MAX_RANGES
        ));
    }
}
