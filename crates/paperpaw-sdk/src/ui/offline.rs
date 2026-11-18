//! The offer to go online, on a screen the reader opened for the network
//! (the store, a catalog, a transfer) while the power mode is Offline
//! (see [`power`](crate::power)): once, inline in the screen's own
//! layout, never as a pop-up over the page: "Offline mode. Go online for
//! this?" with *Go online* and *Not now*.
//!
//! *Go online* sends `Ask::GoOnline`: the
//! network for that screen until it closes; the screen then does again
//! what needed the network. *Not now* leaves the screen as it is without
//! the network (it says so as it would without one). Either way it is
//! not offered again on that screen.
//!
//! Outside a screen of the network (a widget's *Update now*, a plugin's
//! own offer for what its background work would bring) the reader's
//! choice is `Ask::GoOnlineFor`: the network for a while, whatever
//! shows.
//!
//! ```ignore
//! if self.offer.shows(env) {
//!     self.offer.draw(&mut ui, area);
//! }
//! // In `on_action` and `on_gesture`:
//! if let Some(Offer::Online) = self.offer.on_action(action, cx) {
//!     self.fetch(cx);
//! }
//! ```
