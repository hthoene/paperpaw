//! The table check of the `[hyphenation]` file kind, as specified in
//! `docs/abi/proposals/declarative-registration.md` ("Table check at
//! activation"). The contract makes it one function in `paperpaw-hyphen`, used
//! by the kernel and by `paperpaw-dev package`; until that function exists
//! this is the same check written out, so that packages are refused on the host
//! for the reasons the reader will refuse them for.

/// The header of a PPHP version 1 table, in bytes.
const HEADER: usize = 24;

fn u32_at(table: &[u8], at: usize) -> Option<usize> {
    let bytes: [u8; 4] = table.get(at..at.checked_add(4)?)?.try_into().ok()?;
    usize::try_from(u32::from_le_bytes(bytes)).ok()
}

/// Whether `table` is a PPHP version 1 table that a reader can use: the older
/// `QIHP` magic is not accepted; the index is ascending and inside the file;
/// the exceptions walk ends exactly at the end of the file.
pub fn table_check(table: &[u8]) -> Result<(), &'static str> {
    if table.len() < HEADER || &table[..5] != b"PPHP\x01" {
        return Err("not a PPHP version 1 table (the QIHP magic is not accepted)");
    }
    let blocks = u32_at(table, 12).ok_or("no block count")?;
    let exceptions = u32_at(table, 16).ok_or("no exceptions offset")?;
    let count = u32_at(table, 20).ok_or("no exception count")?;
    let entries = blocks
        .checked_mul(4)
        .and_then(|index| index.checked_add(HEADER))
        .ok_or("block count overflows")?;
    if entries > exceptions {
        return Err("the index runs past the exceptions offset");
    }
    if exceptions > table.len() {
        return Err("the exceptions offset is outside the file");
    }
    let mut last: Option<usize> = None;
    for block in 0..blocks {
        let offset = u32_at(table, HEADER + 4 * block).ok_or("the index is cut off")?;
        if last.is_some_and(|before| offset <= before) {
            return Err("index offsets are not ascending");
        }
        last = Some(offset);
        if entries
            .checked_add(offset)
            .is_none_or(|at| at >= exceptions)
        {
            return Err("an index offset points past the patterns");
        }
    }
    let mut at = exceptions;
    for _ in 0..count {
        let length = usize::from(*table.get(at).ok_or("the exceptions are cut off")?);
        let word = table
            .get(at + 1..at + 1 + length)
            .ok_or("an exception is cut off")?;
        if core::str::from_utf8(word).is_err() {
            return Err("an exception is not UTF-8");
        }
        at += 1 + length;
    }
    if at == table.len() {
        Ok(())
    } else {
        Err("the exceptions do not end at the end of the file")
    }
}
