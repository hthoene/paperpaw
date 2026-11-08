//! Allocation that can be refused: the heap of a device is small, and a
//! refusal ends a call with an error, never the plugin.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::StorageError;

/// What a refused allocation is called: the same as a file that does not
/// fit.
const REFUSED: StorageError = StorageError::TooLarge;

/// The parts one after the other as one string.
pub(crate) fn string(parts: &[&str]) -> Result<String, StorageError> {
    let mut out = String::new();
    out.try_reserve_exact(parts.iter().map(|p| p.len()).sum())
        .map_err(|_| REFUSED)?;
    for part in parts {
        out.push_str(part);
    }
    Ok(out)
}

/// `base.n`: the name of the `n`-th file of a kind.
pub(crate) fn numbered(base: &str, n: usize) -> Result<String, StorageError> {
    // Digits of a usize: at most 20.
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    let mut rest = n;
    loop {
        at -= 1;
        digits[at] = b'0' + (rest % 10) as u8;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    let number = core::str::from_utf8(digits.get(at..).unwrap_or_default()).unwrap_or_default();
    string(&[base, ".", number])
}

/// `len` zero bytes.
pub(crate) fn zeroed(len: usize) -> Result<Vec<u8>, StorageError> {
    let mut out = Vec::new();
    out.try_reserve_exact(len).map_err(|_| REFUSED)?;
    out.resize(len, 0);
    Ok(out)
}

/// Room for `more` items, taken at once.
pub(crate) fn reserve<T>(items: &mut Vec<T>, more: usize) -> Result<(), StorageError> {
    items.try_reserve(more).map_err(|_| REFUSED)
}

/// `item` at the end, or an error.
pub(crate) fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), StorageError> {
    reserve(items, 1)?;
    items.push(item);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_strings_are_made_whole() {
        assert_eq!(numbered("run", 0).as_deref(), Ok("run.0"));
        assert_eq!(numbered("run", 1_234_567).as_deref(), Ok("run.1234567"));
        assert_eq!(
            numbered("r", usize::MAX).as_deref(),
            Ok("r.18446744073709551615")
        );
        assert_eq!(string(&["a", "", "bc"]).as_deref(), Ok("abc"));
        assert_eq!(zeroed(3).unwrap(), [0, 0, 0]);
    }

    #[test]
    fn an_impossible_size_is_refused_not_aborted() {
        assert_eq!(zeroed(usize::MAX), Err(StorageError::TooLarge));
        let mut v: Vec<u64> = Vec::new();
        assert_eq!(reserve(&mut v, usize::MAX), Err(StorageError::TooLarge));
    }
}
