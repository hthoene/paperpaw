//! `core.net` for the linked native plugins on the computer: the shared
//! transport (`paperpaw_shell_glue::net`, redirects and all) over the
//! emulator's own client (`http`: the system's `curl` on a thread of its own,
//! or the route a test or the workshop set).

use std::time::Duration;

use paperpaw_core::{HttpCall, HttpResponse};
use paperpaw_shell_glue::net::Transport;

use crate::http::Http;

/// The shell side of `core.net` on the computer.
pub type NativeNet = paperpaw_shell_glue::net::NativeNet<Http>;

impl Transport for Http {
    fn start_unfollowed(&mut self, call: HttpCall) {
        Http::start_unfollowed(self, call);
    }

    fn answer_now(&mut self, response: HttpResponse) {
        Http::answer_now(self, response);
    }

    fn next(&mut self, wait: Option<Duration>) -> Option<HttpResponse> {
        Http::next(self, wait)
    }

    fn in_flight(&self) -> usize {
        self.in_flight
    }
}
