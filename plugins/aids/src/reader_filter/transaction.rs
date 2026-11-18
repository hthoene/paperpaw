//! One bounded transaction; caller scratch and the edit each own half the budget.
use super::{Error, wire};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::document::TextEdit;
use paperpaw_sdk::{
    document::{Align, Block, BlockKind, Run, TextStyle},
    filters::Verdict,
};

pub(super) fn apply(
    batch: &wire::RecordStream<'_>,
    rules: wire::FilterRules<'_>,
    path: &str,
    block: &mut Block,
    budget: usize,
) -> Result<(u64, Verdict), Error> {
    block.try_transform(budget / 2, |edit| {
        if rules.bold == 1 && edit.kind() == BlockKind::Paragraph {
            bold(edit, budget / 2)?;
        }
        for row in batch.clone().skip(1) {
            let Ok(wire::Record::FilterPair(pair)) = row else {
                return Err(Error::Invalid);
            };
            replace(edit, pair.from, pair.to, budget / 2)?;
        }
        let verdict = picture(edit, path, rules.pictures, rules.caption_word, budget / 2)?;
        Ok((rules.signature, verdict))
    })
}

fn replace(edit: &mut TextEdit, from: &str, to: &str, limit: usize) -> Result<(), Error> {
    // Capture boundaries before any edit, exactly as the original filter does.
    // A replacement beside punctuation must not change which earlier matches qualify.
    let matches = || {
        edit.text()
            .match_indices(from)
            .map(|(at, _)| at)
            .filter(|at| crate::whole_word(edit.text(), *at, *at + from.len()))
    };
    let count = matches().count();
    if count
        .checked_mul(core::mem::size_of::<usize>())
        .ok_or(Error::Limit)?
        > limit
    {
        return Err(Error::Limit);
    }
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(count)
        .map_err(|_| Error::NoMemory)?;
    positions.extend(matches());
    for at in positions.into_iter().rev() {
        edit.try_replace(at..at + from.len(), to)?;
    }
    Ok(())
}

fn bold(edit: &mut TextEdit, limit: usize) -> Result<(), Error> {
    let text = edit.text();
    let mut runs = Vec::new();
    let mut start = None;
    let mut letters = 0usize;
    for (at, ch) in text
        .char_indices()
        .chain(core::iter::once((text.len(), ' ')))
    {
        if ch.is_alphabetic() {
            start.get_or_insert(at);
            letters += 1;
        } else if let Some(begin) = start.take() {
            if letters >= 2 {
                let end = text[begin..]
                    .char_indices()
                    .nth(letters.div_ceil(2))
                    .map_or(text.len(), |(offset, _)| begin + offset);
                if !edit
                    .runs()
                    .iter()
                    .any(|r| (r.start as usize) < end && begin < r.end as usize)
                {
                    push(
                        &mut runs,
                        Run::new(begin as u32, end as u32, TextStyle::BOLD),
                        limit,
                    )?;
                }
            }
            letters = 0;
        }
    }
    for run in edit.runs() {
        push(&mut runs, *run, limit)?;
    }
    paperpaw_sdk::sort::sort_by_key(&mut runs, |run| run.start);
    edit.try_set_runs(&runs)?;
    Ok(())
}

fn push(runs: &mut Vec<Run>, run: Run, limit: usize) -> Result<(), Error> {
    // Reserve exactly; include both old and new capacity during allocation.
    let needed = runs.len().checked_add(1).ok_or(Error::Limit)?;
    if needed > runs.capacity() {
        let peak = runs
            .capacity()
            .checked_add(needed)
            .and_then(|n| n.checked_mul(core::mem::size_of::<Run>()))
            .ok_or(Error::Limit)?;
        if peak > limit {
            return Err(Error::Limit);
        }
        runs.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
    }
    runs.push(run);
    Ok(())
}

fn picture(
    edit: &mut TextEdit,
    path: &str,
    policy: u32,
    word: &str,
    limit: usize,
) -> Result<Verdict, Error> {
    let ext = path.rsplit_once('.').map_or("", |(_, ext)| ext);
    if edit.kind() != BlockKind::Image
        || ["cbz", "cbr", "xtc", "xtch"]
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
    {
        return Ok(Verdict::Keep);
    }
    if policy == 2 {
        return Ok(Verdict::Drop);
    }
    if policy != 1 {
        return Ok(Verdict::Keep);
    }
    let alt = edit.text().trim();
    let length = word
        .len()
        .checked_add(alt.len())
        .and_then(|n| n.checked_add(if alt.is_empty() { 2 } else { 4 }))
        .ok_or(Error::Limit)?;
    if length > limit {
        return Err(Error::Limit);
    }
    let mut line = String::new();
    line.try_reserve_exact(length)
        .map_err(|_| Error::NoMemory)?;
    line.push('[');
    line.push_str(word);
    if !alt.is_empty() {
        line.push_str(": ");
        line.push_str(alt);
    }
    line.push(']');
    edit.try_set_text(&line)?;
    edit.try_set_runs(&[Run::new(0, line.len() as u32, TextStyle::ITALIC)])?;
    edit.set_kind(BlockKind::Paragraph);
    edit.set_align(Align::Center);
    edit.drop_image();
    Ok(Verdict::Keep)
}
