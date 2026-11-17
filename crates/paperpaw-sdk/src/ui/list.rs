//! Paged, focusable lists ([`ListView`]) and moving a focus.

/// The focus `delta` steps on from `focus` among `len` things: one step
/// past either end goes round to the other (quicker with few buttons),
/// larger steps (a page, a row of keys) stop at the end. For screens that
/// move a focus of their own, as [`ListView`] does.
pub fn step_focus(focus: usize, delta: isize, len: usize) -> usize {
    let Some(last) = len.checked_sub(1) else {
        return 0;
    };
    let (last, target) = (last as isize, focus as isize + delta);
    match target {
        t if delta.abs() == 1 && t < 0 => last as usize,
        t if delta.abs() == 1 && t > last => 0,
        t => t.clamp(0, last) as usize,
    }
}
