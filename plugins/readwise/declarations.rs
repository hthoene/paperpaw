//! What the Readwise plugin declares: the events it reads. It has no screen,
//! setting or action (it only hears events), so the declarations are the
//! subscriptions: the start (declares the retry of what waits), the end of a
//! request and the work that is due, and the reader's export stage
//! (`reader.notes-exported@1`) the notes it sends arrive with.
use paperpaw_sdk::native::declare::{Declarations, DeclareError, EventKind, SubscribeDecl};

/// Callback tokens: stable within this plugin.
pub const STARTED_TOKEN: u32 = 1;
/// The end of a request (`core.net`).
pub const HTTP_TOKEN: u32 = 2;
/// The work that sends what waits, when it is due.
pub const WORK_TOKEN: u32 = 3;
/// The reader's stage that runs after a book's notes were exported.
pub const EXPORT_TOKEN: u32 = 4;
/// The stage's full id.
pub const EXPORT_STAGE: &str = "reader.notes-exported";

/// The events the plugin reads.
pub fn events<F: FnMut(&[u8]) -> Result<(), i32>>(
    d: &mut Declarations<F>,
) -> Result<(), DeclareError> {
    d.subscribe_event(EventKind::Started, STARTED_TOKEN)?;
    d.subscribe_event(EventKind::HttpDone, HTTP_TOKEN)?;
    d.subscribe_event(EventKind::WorkDue, WORK_TOKEN)?;
    d.subscribe(SubscribeDecl::new(EXPORT_STAGE, EXPORT_TOKEN))?;
    d.submit()?;
    Ok(())
}
