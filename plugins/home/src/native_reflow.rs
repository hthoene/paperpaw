//! The pages fitted to a new grid: when the grid's columns or rows change, each
//! layout is fitted to its own grid, what has no room goes to the next page
//! and nothing is lost (`native_pages::refit`). The writes are staged in the
//! callback of the setting's change, with the dashboard's own budget check.
use paperpaw_sdk::native::{Error, Host};

use crate::native_add_screen::apply;
use crate::native_pages::refit;
use crate::native_shown::Shown;

/// Fits the pages to the grid as it is now; `true` where something was
/// written, `false` where nothing moved or the change was over the budget of a
/// callback (the old layout stays and the reader is told).
///
/// # Errors
/// Allocation and host errors; an error return drops everything staged.
pub fn fit(host: &Host<'_>) -> Result<bool, Error> {
    let shown = Shown::current(host)?;
    let writes = refit(&shown.src, &shown.grid(false), &shown.grid(true))?;
    if writes.is_empty() {
        return Ok(false);
    }
    apply(host, &writes, &[])
}
