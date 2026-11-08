//! Allocation that reports refusal. The standard collections abort the
//! device when the heap refuses; every allocation on a callback's path goes
//! through here instead and ends in `Error::NoMemory`.
use alloc::{string::String, vec::Vec};
use core::fmt::{self, Write};
use paperpaw_sdk::native::Error;

/// Grows `vec` by room for one item, then pushes it.
pub(crate) fn push<T>(vec: &mut Vec<T>, item: T) -> Result<(), Error> {
    vec.try_reserve(1).map_err(|_| Error::NoMemory)?;
    vec.push(item);
    Ok(())
}

/// Reserves `count` more items in one step, so that later pushes cannot fail.
pub(crate) fn reserve<T>(vec: &mut Vec<T>, count: usize) -> Result<(), Error> {
    vec.try_reserve_exact(count).map_err(|_| Error::NoMemory)
}

/// Appends `text`.
pub(crate) fn push_str(string: &mut String, text: &str) -> Result<(), Error> {
    string
        .try_reserve(text.len())
        .map_err(|_| Error::NoMemory)?;
    string.push_str(text);
    Ok(())
}

/// Appends `c`.
pub(crate) fn push_char(string: &mut String, c: char) -> Result<(), Error> {
    string
        .try_reserve(c.len_utf8())
        .map_err(|_| Error::NoMemory)?;
    string.push(c);
    Ok(())
}

/// An owned copy of `text`.
pub(crate) fn string(text: &str) -> Result<String, Error> {
    let mut copy = String::new();
    push_str(&mut copy, text)?;
    Ok(copy)
}

/// The strings, with `separator` between them.
pub(crate) fn join(parts: &[String], separator: &str) -> Result<String, Error> {
    let mut length = separator
        .len()
        .saturating_mul(parts.len().saturating_sub(1));
    for part in parts {
        length = length.checked_add(part.len()).ok_or(Error::NoMemory)?;
    }
    let mut whole = String::new();
    whole
        .try_reserve_exact(length)
        .map_err(|_| Error::NoMemory)?;
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            whole.push_str(separator);
        }
        whole.push_str(part);
    }
    Ok(whole)
}

struct Writer<'a>(&'a mut String);

impl Write for Writer<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0.try_reserve(text.len()).map_err(|_| fmt::Error)?;
        self.0.push_str(text);
        Ok(())
    }
}

/// `format!` that reports a refused allocation. Only numbers and borrowed
/// text are formatted with it, which cannot fail in any other way.
pub(crate) fn formatted(arguments: fmt::Arguments<'_>) -> Result<String, Error> {
    let mut text = String::new();
    Writer(&mut text)
        .write_fmt(arguments)
        .map_err(|_| Error::NoMemory)?;
    Ok(text)
}

/// `format!`, fallibly: `mem::format!("{a}{b}")` is a `Result<String, Error>`.
macro_rules! format {
    ($($argument:tt)*) => {
        $crate::mem::formatted(::core::format_args!($($argument)*))
    };
}
pub(crate) use format;

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use crate::log::{self, Kind, Reading, Sessions};
    use core::{alloc::GlobalAlloc, cell::Cell};
    use std::alloc::{Layout, System};

    std::thread_local! {
        /// The allocation to refuse, counted from 1 (0: none), and how many
        /// this thread has made since.
        static REFUSE_AT: Cell<usize> = const { Cell::new(0) };
        static COUNT: Cell<usize> = const { Cell::new(0) };
    }

    struct Failing;

    fn refuse() -> bool {
        let at = REFUSE_AT.try_with(Cell::get).unwrap_or(0);
        at != 0
            && COUNT
                .try_with(|count| {
                    count.set(count.get() + 1);
                    count.get() == at
                })
                .unwrap_or(false)
    }

    // SAFETY: forwards to `System`, or returns null to refuse.
    unsafe impl GlobalAlloc for Failing {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if refuse() {
                return core::ptr::null_mut();
            }
            // SAFETY: the caller's layout, forwarded.
            unsafe { System.alloc(layout) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: allocated by `alloc` above.
            unsafe { System.dealloc(ptr, layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            if refuse() {
                return core::ptr::null_mut();
            }
            // SAFETY: allocated by this allocator with this layout.
            unsafe { System.realloc(ptr, layout, size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Failing = Failing;

    /// Runs `run` once counting its allocations, then again with each of them
    /// refused: a refusal is `NoMemory`, never an abort, and never a value.
    fn sweep<T>(mut run: impl FnMut() -> Result<T, Error>) -> usize {
        COUNT.with(|count| count.set(0));
        REFUSE_AT.with(|at| at.set(usize::MAX));
        let first = run();
        let total = COUNT.with(Cell::get);
        REFUSE_AT.with(|at| at.set(0));
        assert!(first.is_ok());
        drop(first);
        for refused in 1..=total {
            COUNT.with(|count| count.set(0));
            REFUSE_AT.with(|at| at.set(refused));
            let result = run();
            REFUSE_AT.with(|at| at.set(0));
            assert_eq!(result.err(), Some(Error::NoMemory), "allocation {refused}");
        }
        total
    }

    #[test]
    fn every_helper_reports_a_refused_allocation() {
        assert!(sweep(|| string("some text")) >= 1);
        assert!(sweep(|| join(&[string("a")?, string("b")?], " · ")) >= 3);
        assert!(sweep(|| self::format!("{}-{:02} V", 12, 3)) >= 1);
        assert!(
            sweep(|| {
                let mut items = Vec::new();
                push(&mut items, 1)?;
                reserve(&mut items, 64)?;
                Ok(items)
            }) >= 2
        );
        assert!(
            sweep(|| {
                let mut text = String::new();
                push_char(&mut text, '…')?;
                push_str(&mut text, "tail")?;
                Ok(text)
            }) >= 1
        );
    }

    #[test]
    fn a_log_line_reports_a_refused_allocation() {
        let reading = Reading {
            ms: 5,
            millivolts: Some(4012),
            percent: Some(81),
            unix: Some(1_791_974_460),
            ..Reading::default()
        };
        assert!(sweep(|| log::try_line(Kind::Sample, &reading)) >= 1);
    }

    #[test]
    fn reading_the_history_allocates_once_for_all_times_awake() {
        let mut sessions = Sessions::try_new().unwrap();
        COUNT.with(|count| count.set(0));
        REFUSE_AT.with(|at| at.set(usize::MAX));
        for index in 0..300u64 {
            let at = Reading {
                ms: index * 1000,
                ..Reading::default()
            };
            sessions.feed(Kind::Start, at);
            sessions.feed(Kind::End, at);
        }
        let made = COUNT.with(Cell::get);
        REFUSE_AT.with(|at| at.set(0));
        assert_eq!(made, 0, "pushing past the kept times awake must not grow");
        assert_eq!(sessions.ended.len(), 64);
    }
}
