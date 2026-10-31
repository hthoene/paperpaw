//! Any text and any options wrapped into lines, greedily and balanced, at
//! widths the input picks: no panic, no endless loop (counted: a wrapper
//! that does not move forward trips the assertion at once), and the lines
//! tile the text in order (A5, designs/line-breaking-a5.md, R2-1).

#![no_main]

use std::cell::Cell;

use libfuzzer_sys::fuzz_target;
use paperpaw_text::{Line, wrap_lines_balanced_opts, wrap_lines_kerned_opts};

fn check(lines: &[Line], text: &str) {
    let mut at = 0;
    for line in lines {
        assert!(line.range.start >= at && line.range.end >= line.range.start);
        assert!(text.is_char_boundary(line.range.start) && text.is_char_boundary(line.range.end));
        at = line.range.end;
    }
    assert!(at <= text.len());
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 6 {
        return;
    }
    let options = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let first = 8.0 + f32::from(data[4]);
    let rest = 8.0 + f32::from(data[5]);
    let text = String::from_utf8_lossy(&data[6..]);
    let text: &str = &text;
    let budget = Cell::new(200 * text.len() + 10_000);
    let advance = |_: usize, c: char| {
        let left = budget.get();
        assert!(left > 0, "the wrapper does not move forward: {text:?} {options:#x}");
        budget.set(left - 1);
        match c {
            ' ' => 4.0,
            c if (c as u32) < 0x300 => 6.0,
            _ => 10.0,
        }
    };
    let width = |n: usize| if n == 0 { first } else { rest };
    let lines = wrap_lines_kerned_opts(text, width, advance, |_, _, _| 0.0, None, options);
    check(&lines, text);
    let width = |_: usize| rest;
    let lines = wrap_lines_balanced_opts(text, width, advance, |_, _, _| 0.0, None, |_, _| 0.0, options);
    check(&lines, text);
});
