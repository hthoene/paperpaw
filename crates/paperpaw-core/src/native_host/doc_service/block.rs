//! Op 6 of the document service: the block's question, its answer checked
//! (`document.md` 4), its foreign elements replaced by the hooks' texts
//! (`splice`) and its pictures given the opener's ids.
use super::*;

/// Op 6: the block at `at`, or the next fragment of the one in progress.
pub(super) fn block_ask(
    r: &wire::ServiceDocAt,
    document: &Document,
    call: &mut FormatCall<'_>,
) -> Result<Check, i32> {
    // `insert` is the reader's own (inserted content); never here.
    require(r.insert == 0)?;
    args::page_state(r.cursor, r.generation)?;
    // A later fragment continues the block the last call began: the table
    // keeps what the earlier fragments said (`document.md` 4).
    let resume = if r.cursor == 0 {
        None
    } else {
        let pending = document.block.ok_or(ERR_STATE)?;
        require_state(pending.is(r.section, r.offset, r.generation, r.cursor))?;
        Some(pending.fragment)
    };
    call.op = fmt::BLOCK;
    call.section = r.section;
    call.offset = r.offset;
    call.cursor = r.cursor;
    call.generation = r.generation;
    Ok(Check::Block(BlockAt {
        section: r.section,
        offset: r.offset,
        cursor: r.cursor,
        generation: r.generation,
        resume,
    }))
}

/// Op 6's answer: checked as the next fragment, then remembered when the block
/// goes on and forgotten when it ends. Only a fragment that passed moves it.
pub(super) fn block_verify(
    host: &dyn DocHost,
    document: &Document,
    at: BlockAt,
    out: &mut [u8],
    length: usize,
) -> Result<usize, i32> {
    let (end, fragment) = check::block(
        &out[..length],
        at.cursor,
        at.generation,
        at.resume,
        |_, _| Ok(()),
    )?;
    if end.flags & 2 == 0 && fragment.picture_missing() {
        host.picture_left_out(document.id);
    }
    // A foreign element goes, replaced by its hook's text where the block is
    // whole in this fragment (`document.md` 4.1).
    let whole = at.cursor == 0 && end.flags & 2 == 0;
    let length = splice::foreign(host, whole, out, length)?;
    let kept = images(host, document, out, length, true)?;
    let pending = (end.flags & 2 != 0).then_some(table::Pending {
        section: at.section,
        offset: at.offset,
        generation: end.generation,
        next: end.next,
        fragment,
    });
    host.table().keep_block(document.id, pending);
    Ok(kept)
}

/// Replace each format picture by the opener's image id, in place. A picture
/// the host cannot vouch for is left out of the stream (its block keeps its
/// alt text), never failing the stream. Returns the new length.
pub(super) fn images(
    host: &dyn DocHost,
    document: &Document,
    out: &mut [u8],
    mut length: usize,
    in_block: bool,
) -> Result<usize, i32> {
    let mut at = 0;
    while at < length {
        let (raw, used) = wire::RawRecord::prefix(&out[at..length])?;
        if raw.tag() != wire::SERVICE_DOC_IMAGE_TAG {
            at += used;
            continue;
        }
        let image: wire::ServiceDocImage = raw.decode()?;
        match host.map_image(document, &image, in_block)? {
            Some(id) => {
                // The same fixed-width record: it fits where the old one was.
                let mut scratch = [0_u8; 32];
                let size = encode(
                    &Record::ServiceDocImage(wire::ServiceDocImage { image: id, ..image }),
                    &mut scratch,
                )?;
                require(size == used)?;
                out[at..at + used].copy_from_slice(&scratch[..size]);
                at += used;
            }
            None => {
                if in_block {
                    host.picture_left_out(document.id);
                }
                out.copy_within(at + used..length, at);
                length -= used;
            }
        }
    }
    Ok(length)
}
