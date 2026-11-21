//! The offer to go online where Offline mode held a request or socket back
//! (its end said `Reason::Offline`): Beta 3's `ui::offline::OfflineOffer`
//! for plugins with code (#459). The host asks "This needs the network. Go
//! online for it?" with *Go online* and *Not now*, in the core's words (every
//! language PaperPaw has them), once for what the reader started.
//!
//! *Go online* sends `command.go_online` (the plugin declares `uses.wifi`)
//! from the answer, inside the host's window after an input, and hands back
//! what is to be done again; *Not now* (or *Back*) hands it back as well, for
//! the plugin to say that it needs the network.
//!
//! The host goes online only once the answer's callback has returned (its
//! commands run then), so what is done again from the answer still meets
//! Offline mode and ends `Offline`: that end, the first after *Go online*, is
//! where [`Offer::again`] says to do it once more, and the network is there by
//! then. Where the host refused to go online, that one ends `Offline` too and,
//! offered already, says so instead of asking again.
//!
//! ```ignore
//! // Where a request ended `Offline`:
//! if self.online.again() {
//!     return self.start(host, Again::Fetch);
//! }
//! if self.online.may_offer() {
//!     return self.online.ask(host, &self.ui, Again::Fetch);
//! }
//! // On `Event::Answered`:
//! match self.online.answered(host, answer)? {
//!     Some(Chosen::Online(again)) => self.start(host, again),
//!     Some(Chosen::NotNow(_)) => self.say(host, "plugin.no-network"),
//!     None => {} // not the offer's answer
//! }
//! ```
use crate::{
    Messages,
    mem::{SERVICE_BUFFER, buffer},
    text::concat,
};
use paperpaw_sdk::native::{
    Error, Host,
    commands::CommandSender,
    events::{AnswerOutcome, Answered},
    ui::{Question, Ui},
};

/// How long the network is asked for (5 minutes, as the store and the fonts'
/// browser ask).
pub const GO_ONLINE_MS: u32 = 300_000;

/// What the reader answered to the offer, with what is to be done again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen<T> {
    /// *Go online*: the network was asked for; do it again now.
    Online(T),
    /// *Not now* or *Back*: say that it needs the network.
    NotNow(T),
}

/// The offer: asked at most once until [`Offer::reset`], and the question
/// open with what it is for.
#[derive(Debug)]
pub struct Offer<T> {
    /// The open question's request (the host's number) and what it is for.
    asked: Option<(u32, T)>,
    offered: bool,
    /// *Go online* was chosen and nothing has ended `Offline` since.
    went: bool,
}

impl<T> Default for Offer<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Offer<T> {
    /// Not offered yet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            asked: None,
            offered: false,
            went: false,
        }
    }

    /// Whether the offer may be made: not made since the reader started what
    /// Offline mode held back, and no question open.
    #[must_use]
    pub const fn may_offer(&self) -> bool {
        !self.offered && self.asked.is_none()
    }

    /// The reader started something anew: it may be offered again.
    pub fn reset(&mut self) {
        self.offered = false;
        self.went = false;
    }

    /// Whether what ended `Offline` is to be done once more: true once, for
    /// the first such end after *Go online* (what was done again from the
    /// answer, before the host went online).
    pub fn again(&mut self) -> bool {
        core::mem::take(&mut self.went)
    }

    /// Asks the host's question, for `again`.
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals (nothing is asked then, and the
    /// offer stays possible).
    pub fn ask(&mut self, host: &Host<'_>, ui: &Ui, again: T) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "core.offline.title")?;
        let text = messages.copy(host, "core.offline.text")?;
        let labels = concat(&[
            &messages.copy(host, "core.offline.go-online")?,
            "\n",
            &messages.copy(host, "core.offline.not-now")?,
        ])?;
        let question = Question::confirm(&title, &text)?
            .confirm_labels(&labels)?
            .focus_yes()?;
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let request = ui.ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request.get(), again));
        self.offered = true;
        Ok(())
    }

    /// The host's answer to a question: `None` where it was not the offer's.
    /// On *Go online* the network is asked for before it returns.
    ///
    /// # Errors
    /// `NoMemory` for the command (the choice is kept for no second answer:
    /// what was to be done again is dropped). Any other refusal of going
    /// online is left to what is done again, which then ends `Offline`.
    pub fn answered(
        &mut self,
        host: &Host<'_>,
        answer: Answered,
    ) -> Result<Option<Chosen<T>>, Error> {
        let chosen = self.take(answer.request().get(), answer.outcome(), answer.choice());
        if let Some(Chosen::Online(_)) = chosen {
            let mut scratch = [0; 64];
            if let Err(Error::NoMemory) =
                CommandSender::new(host, &mut scratch).go_online(GO_ONLINE_MS)
            {
                return Err(Error::NoMemory);
            }
        }
        Ok(chosen)
    }

    /// The decision on an answer to request `request`: the offer's question
    /// closes on an answer or a cancel, never on live typing.
    fn take(&mut self, request: u32, outcome: AnswerOutcome, choice: u32) -> Option<Chosen<T>> {
        let ours = matches!(&self.asked, Some((asked, _)) if *asked == request);
        if !ours || outcome == AnswerOutcome::Typing {
            return None;
        }
        let (_, again) = self.asked.take()?;
        Some(if outcome == AnswerOutcome::Answered && choice == 1 {
            self.went = true;
            Chosen::Online(again)
        } else {
            Chosen::NotNow(again)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{AnswerOutcome, Chosen, Offer};

    /// An offer whose question is open as request `request`, as `Offer::ask`
    /// leaves it.
    fn asked(request: u32) -> Offer<&'static str> {
        let mut offer = Offer::new();
        offer.asked = Some((request, "fetch"));
        offer.offered = true;
        offer
    }

    #[test]
    fn go_online_hands_back_what_to_do_again_and_the_offer_is_made_once() {
        let mut offer = asked(7);
        assert!(!offer.may_offer(), "a question is open");
        assert_eq!(
            offer.take(7, AnswerOutcome::Answered, 1),
            Some(Chosen::Online("fetch"))
        );
        assert!(!offer.may_offer(), "once until the reader starts anew");
        offer.reset();
        assert!(offer.may_offer());
    }

    #[test]
    fn the_first_offline_end_after_go_online_is_done_once_more() {
        let mut offer = asked(7);
        assert!(!offer.again(), "not chosen yet");
        offer.take(7, AnswerOutcome::Answered, 1);
        assert!(offer.again(), "what was done again from the answer");
        assert!(!offer.again(), "once: held back again, it says so");
        let mut offer = asked(7);
        offer.take(7, AnswerOutcome::Answered, 1);
        offer.reset();
        assert!(!offer.again(), "the reader started anew");
    }

    #[test]
    fn not_now_and_back_hand_it_back_to_say_it_needs_the_network() {
        let mut offer = asked(7);
        assert_eq!(
            offer.take(7, AnswerOutcome::Answered, 0),
            Some(Chosen::NotNow("fetch"))
        );
        assert!(!offer.again(), "nothing is done once more");
        let mut offer = asked(7);
        assert_eq!(
            offer.take(7, AnswerOutcome::Cancelled, 1),
            Some(Chosen::NotNow("fetch"))
        );
    }

    #[test]
    fn another_question_and_live_typing_are_not_the_offers() {
        let mut offer = asked(7);
        assert_eq!(offer.take(8, AnswerOutcome::Answered, 1), None);
        assert_eq!(offer.take(7, AnswerOutcome::Typing, 1), None);
        assert_eq!(
            offer.take(7, AnswerOutcome::Answered, 1),
            Some(Chosen::Online("fetch")),
            "still open"
        );
        assert_eq!(offer.take(7, AnswerOutcome::Answered, 1), None, "closed");
    }
}
