//! Fallible allocation helpers: every growth is reserved first, so a refused
//! allocation is [`Error::NoMemory`] and never an abort.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;

/// Reply capacity every `core.ui` operation accepts, and so the size of the
/// buffers handed to its calls.
pub const SERVICE_BUFFER: usize = 4096;

/// A zeroed buffer of `len` bytes.
///
/// # Errors
/// [`Error::NoMemory`] when it cannot be allocated.
pub fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// `value` on the heap: a refused allocation is [`Error::NoMemory`], never an
/// abort (`Box::new` aborts). For state a plugin keeps beside a small struct
/// of its own, made when it is first needed.
///
/// # Errors
/// [`Error::NoMemory`] when it cannot be allocated.
pub fn boxed<T>(value: T) -> Result<alloc::boxed::Box<T>, Error> {
    let layout = core::alloc::Layout::new::<T>();
    if layout.size() == 0 {
        return Ok(alloc::boxed::Box::new(value));
    }
    // SAFETY: the layout is not zero-sized; a null result is refused; the
    // block is then written with a `T` and handed to a `Box`, which frees it
    // with the same layout (the global allocator's).
    unsafe {
        let block = alloc::alloc::alloc(layout).cast::<T>();
        if block.is_null() {
            return Err(Error::NoMemory);
        }
        block.write(value);
        Ok(alloc::boxed::Box::from_raw(block))
    }
}

/// Fills `bytes` with zeros in a way the compiler cannot drop as a dead store
/// before the memory is freed: for a password, a key or a typed secret.
pub fn wipe(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: each byte is live and exclusively borrowed; the volatile
        // store keeps the wipe when the allocation is freed right after.
        unsafe { core::ptr::write_volatile(byte, 0) };
    }
    core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
}

/// Zeroes `text` and frees it.
pub fn wipe_text(text: String) {
    let mut bytes = text.into_bytes();
    wipe(&mut bytes);
}

/// The least a full vector grows by at one push.
const GROW_LEAST: usize = 16;

/// Appends `item` after reserving room for it. A full vector grows by its
/// length up to 16 elements, and by an eighth of its length where that is more,
/// not by doubling: a list of a few hundred never holds the half again that
/// doubling leaves empty (at most an eighth and 16 spare), while a long one
/// still reallocates only a few times per doubling (copying stays linear). Where
/// that does not fit, it grows by one.
///
/// # Errors
/// [`Error::NoMemory`] when the vector cannot grow; it is left unchanged.
pub fn push<T>(items: &mut Vec<T>, item: T) -> Result<(), Error> {
    if items.len() == items.capacity() {
        let more = items.len().clamp(1, GROW_LEAST).max(items.len() / 8);
        if items.try_reserve_exact(more).is_err() {
            items.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
        }
    }
    items.push(item);
    Ok(())
}

/// The items of a fallible iterator, collected with reserved growth.
///
/// # Errors
/// The iterator's first error, or [`Error::NoMemory`].
pub fn collect<T>(items: impl IntoIterator<Item = Result<T, Error>>) -> Result<Vec<T>, Error> {
    let mut found = Vec::new();
    for item in items {
        push(&mut found, item?)?;
    }
    Ok(found)
}

/// Appends the items of `more` with reserved growth.
///
/// # Errors
/// [`Error::NoMemory`]; items appended before the refusal stay.
pub fn extend<T>(items: &mut Vec<T>, more: impl IntoIterator<Item = T>) -> Result<(), Error> {
    for item in more {
        push(items, item)?;
    }
    Ok(())
}

/// A copy of `items`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy_strings(items: &[String]) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        found.push(crate::text::copy(item)?);
    }
    Ok(found)
}

/// `text` in lower case, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn lowercase(text: &str) -> Result<String, Error> {
    let mut result = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        result
            .try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        result.push(c);
    }
    Ok(result)
}

/// `items` joined by `separator`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn join(items: &[String], separator: &str) -> Result<String, Error> {
    let mut len = 0usize;
    for (at, item) in items.iter().enumerate() {
        if at > 0 {
            len = len.checked_add(separator.len()).ok_or(Error::NoMemory)?;
        }
        len = len.checked_add(item.len()).ok_or(Error::NoMemory)?;
    }
    let mut result = String::new();
    result.try_reserve_exact(len).map_err(|_| Error::NoMemory)?;
    for (at, item) in items.iter().enumerate() {
        if at > 0 {
            result.push_str(separator);
        }
        result.push_str(item);
    }
    Ok(result)
}

/// `text` with every `from` character replaced by `to`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn replace_char(text: &str, from: char, to: char) -> Result<String, Error> {
    let mut result = String::new();
    result
        .try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    for c in text.chars() {
        // Both are ASCII where this is used; a wider `to` could outgrow the reservation.
        let c = if c == from { to } else { c };
        result
            .try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        result.push(c);
    }
    Ok(result)
}

/// Sorts `items` stably, in the shared sorter ([`paperpaw_sdk::sort`]):
/// order is that of `sort_by` with the same comparison. Up to 12 items it
/// allocates nothing; beyond, it asks for at most `4 * items.len()` bytes and
/// sorts in place when the heap refuses them. Where `compare` is no total
/// order the order is not said, but no item is lost or repeated and nothing
/// panics.
pub fn stable_sort_by<T>(items: &mut [T], compare: impl FnMut(&T, &T) -> core::cmp::Ordering) {
    paperpaw_sdk::sort::sort_by(items, compare);
}

/// A vector holding `item` alone, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn one<T>(item: T) -> Result<Vec<T>, Error> {
    let mut found = Vec::new();
    found.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
    found.push(item);
    Ok(found)
}

/// The text of each of `items`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn strs(items: &[String]) -> Result<Vec<&str>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    for item in items {
        found.push(item.as_str());
    }
    Ok(found)
}

/// A copy of `items`, with fallible allocation.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn copy_strs<'a>(items: &[&'a str]) -> Result<Vec<&'a str>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(items.len())
        .map_err(|_| Error::NoMemory)?;
    found.extend_from_slice(items);
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boxed_value_is_kept_whole_and_dropped_once() {
        use core::sync::atomic::{AtomicUsize, Ordering};
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct Big([u8; 600], u32);
        impl Drop for Big {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }
        let mut bytes = [0u8; 600];
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let boxed = boxed(Big(bytes, 7)).unwrap();
        assert_eq!(
            (boxed.1, boxed.0[0], boxed.0[599]),
            (7, 0, (599 % 251) as u8)
        );
        assert_eq!(DROPS.load(Ordering::SeqCst), 0);
        drop(boxed);
        assert_eq!(DROPS.load(Ordering::SeqCst), 1);
        // A value with no size needs no block.
        assert_eq!(*super::boxed(()).unwrap(), ());
        // A value aligned wider than a word keeps its alignment.
        #[repr(align(64))]
        struct Wide(u8);
        let wide = super::boxed(Wide(9)).unwrap();
        assert_eq!((wide.0, core::ptr::from_ref(&*wide) as usize % 64), (9, 0));
    }

    #[test]
    fn a_full_vector_grows_by_its_length_up_to_a_few_and_never_doubles() {
        let mut items: alloc::vec::Vec<u32> = alloc::vec::Vec::new();
        let mut most_spare = 0;
        for n in 0..300 {
            push(&mut items, n).unwrap();
            most_spare = most_spare.max(items.capacity() - items.len());
        }
        assert_eq!(items.len(), 300);
        assert!(items.iter().copied().eq(0..300));
        // Doubling would leave up to 255 spare at 300; an eighth and 16 leave 53.
        assert!(most_spare <= 300 / 8 + GROW_LEAST, "{most_spare}");
    }

    #[test]
    fn a_long_vector_is_copied_a_few_times_not_every_sixteen_pushes() {
        let mut items: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        let (mut moves, mut at) = (0, core::ptr::null());
        for n in 0..65_536_u32 {
            push(&mut items, n as u8).unwrap();
            if items.as_ptr() != at {
                moves += 1;
                at = items.as_ptr();
            }
        }
        // About 8 growths per doubling over 12 doublings, not 4,096.
        assert!(moves < 200, "{moves}");
    }

    #[test]
    fn lowercase_matches_the_standard_conversion_for_plain_text() {
        assert_eq!(lowercase("Ärger ÜBER Ǆ").unwrap(), "ärger über ǆ");
    }

    #[test]
    fn the_sort_is_stable_and_matches_the_standard_one() {
        let mut items: alloc::vec::Vec<(u8, u8)> =
            [(2, 0), (1, 1), (2, 2), (1, 3), (0, 4), (2, 5)].into();
        let mut expected = items.clone();
        expected.sort_by_key(|a| a.0);
        stable_sort_by(&mut items, |a, b| a.0.cmp(&b.0));
        assert_eq!(items, expected);
    }

    #[test]
    fn longer_lists_are_sorted_stably_like_the_standard_sort() {
        let mut seed = 5_u32;
        for len in [13_usize, 40, 257, 1_000] {
            let mut items: alloc::vec::Vec<(u8, u32)> = (0..len as u32)
                .map(|at| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    // Few distinct keys: stability decides the order.
                    ((seed >> 28) as u8 % 5, at)
                })
                .collect();
            let mut expected = items.clone();
            expected.sort_by_key(|a| a.0);
            stable_sort_by(&mut items, |a, b| a.0.cmp(&b.0));
            assert_eq!(items, expected, "{len} items");
        }
    }

    #[test]
    fn join_and_replace_build_what_the_standard_ones_do() {
        let items = [String::from("a"), String::from("b")];
        assert_eq!(join(&items, ",").unwrap(), "a,b");
        assert_eq!(join(&[], ",").unwrap(), "");
        assert_eq!(replace_char("a-b-c", '-', ' ').unwrap(), "a b c");
    }

    #[test]
    fn wipe_zeroes_what_it_is_given() {
        let mut bytes = *b"hunter2";
        wipe(&mut bytes);
        assert_eq!(bytes, [0; 7]);
        wipe(&mut []);
        wipe_text(String::from("hunter2"));
    }

    #[test]
    fn collect_stops_at_the_first_error() {
        let items = [Ok(1), Err(Error::Invalid), Ok(3)];
        assert_eq!(collect(items), Err(Error::Invalid));
        assert_eq!(collect([Ok(1), Ok(2)]).unwrap(), [1, 2]);
    }

    #[test]
    fn helpers_survive_every_refused_allocation() {
        let items = [String::from("alpha"), String::from("beta")];
        crate::fail_alloc::sweep(|| buffer(64).map(|b| b.len()));
        crate::fail_alloc::sweep(|| copy_strings(&items).map(|v| v.len()));
        crate::fail_alloc::sweep(|| join(&items, ",").map(|s| s.len()));
        crate::fail_alloc::sweep(|| lowercase("ÄRGER Über").map(|s| s.len()));
        crate::fail_alloc::sweep(|| replace_char("a-b-c", '-', ' ').map(|s| s.len()));
        crate::fail_alloc::sweep(|| collect((0..40).map(Ok)).map(|v| v.len()));
    }
}
