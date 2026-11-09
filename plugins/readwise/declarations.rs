//! What the Readwise plugin declares: the events it reads. It has no screen,
//! setting or action (it only hears events), so the declarations are the
//! subscriptions: the start (declares the retry of what waits), the end of a
//! request and the work that is due. The notes it sends arrive with the
//! export stage of the reader, subscribed to once the reader defines it
//! (`PORTING-7.md`).
use paperpaw_sdk::native::declare::{Declarations, DeclareError, EventKind};

/// Callback tokens: stable within this plugin.
pub const STARTED_TOKEN: u32 = 1;
/// The end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 2;
/// The work that sends what waits, when it is due.
pub const WORK_TOKEN: u32 = 3;

/// The events the plugin reads.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED_TOKEN)?;
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.submit()?;
    Ok(())
}
