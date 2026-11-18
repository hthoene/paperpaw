//! The compose window, host-free: its rows, what each says, what a typed answer
//! changes, why a draft is refused, a fresh draft and the id of a message.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

use paperpaw_plugin_mail::native_model::browser::Words;
use paperpaw_plugin_mail::native_model::compose::{Draft, Problem};
use paperpaw_plugin_mail::native_model::mem::Error;
use paperpaw_plugin_mail::native_model::writer::{
    Part, current, fresh_body, header_name, message_id, parts, problem_key, typed, value,
};

struct Said;

impl Words for Said {
    fn tr(&mut self, key: &str) -> Result<String, Error> {
        Ok(format!("<{key}>"))
    }
    fn when(&mut self, _: i64) -> Result<String, Error> {
        Ok(String::new())
    }
}

#[test]
fn the_sender_is_a_row_only_where_there_are_several_accounts() {
    assert_eq!(
        parts(1).unwrap(),
        [Part::To, Part::Cc, Part::Subject, Part::Body, Part::Send]
    );
    assert_eq!(parts(2).unwrap()[0], Part::From);
    assert_eq!(parts(3).unwrap().len(), 6);
    assert_eq!(parts(0).unwrap().len(), 5);
}

#[test]
fn a_row_asks_for_text_unless_it_sends_or_picks() {
    assert!(Part::To.typed() && Part::Cc.typed() && Part::Subject.typed() && Part::Body.typed());
    assert!(!Part::From.typed() && !Part::Send.typed());
}

#[test]
fn the_rows_say_what_was_typed() {
    let mut draft = Draft::with_sender("Ada", "ada@example.org").unwrap();
    typed(&mut draft, Part::To, "  bob@example.org \n").unwrap();
    typed(&mut draft, Part::Cc, "").unwrap();
    typed(&mut draft, Part::Subject, " Hi ").unwrap();
    typed(&mut draft, Part::Body, "\n\nFirst line\nSecond\n").unwrap();
    let said = |draft: &Draft, part| value(part, draft, "ada@example.org", &mut Said).unwrap();
    assert_eq!(said(&draft, Part::From), "ada@example.org");
    assert_eq!(said(&draft, Part::To), "bob@example.org");
    assert_eq!(said(&draft, Part::Cc), "");
    assert_eq!(said(&draft, Part::Subject), "Hi");
    // The first line with something in it; the text itself is kept whole.
    assert_eq!(said(&draft, Part::Body), "First line");
    assert_eq!(draft.body, "\n\nFirst line\nSecond\n");
    assert_eq!(said(&draft, Part::Send), "");
    typed(&mut draft, Part::Body, " \n\n").unwrap();
    assert_eq!(said(&draft, Part::Body), "<mail.body.empty>");
    // Typing in a row that is not typed changes nothing.
    typed(&mut draft, Part::Send, "x").unwrap();
    assert_eq!(draft.subject, "Hi");
}

#[test]
fn a_question_starts_from_what_is_there() {
    let mut draft = Draft::with_sender("", "a@x.org").unwrap();
    typed(&mut draft, Part::To, "b@x.org").unwrap();
    typed(&mut draft, Part::Body, "text").unwrap();
    assert_eq!(current(&draft, Part::To), "b@x.org");
    assert_eq!(current(&draft, Part::Body), "text");
}

#[test]
fn every_problem_has_its_message() {
    for (problem, key) in [
        (Problem::NoSender, "mail.problem.sender"),
        (Problem::NoRecipient, "mail.problem.recipient"),
        (Problem::BadAddress, "mail.problem.address"),
        (Problem::TooManyRecipients, "mail.problem.many"),
        (Problem::TooLong, "mail.problem.long"),
    ] {
        assert_eq!(problem_key(problem), key);
    }
}

#[test]
fn a_fresh_draft_has_the_signature_or_nothing() {
    assert_eq!(fresh_body("").unwrap(), "");
    assert_eq!(fresh_body("  \n").unwrap(), "");
    assert_eq!(fresh_body(" Ada \n").unwrap(), "\n\n-- \nAda\n");
}

#[test]
fn a_sender_is_shown_by_name_else_by_address() {
    assert_eq!(header_name("Jane Doe <jane@x.org>").unwrap(), "Jane Doe");
    assert_eq!(header_name("jane@x.org").unwrap(), "jane@x.org");
    assert_eq!(header_name("").unwrap(), "");
}

#[test]
fn an_id_is_random_hex_or_the_time() {
    assert_eq!(
        message_id(Some(&[0x00, 0xab, 0xff, 0x10]), 5).unwrap(),
        "00abff10"
    );
    assert_eq!(message_id(None, 1_793_693_700).unwrap(), "1793693700");
    assert_eq!(message_id(Some(&[]), 7).unwrap(), "7");
}
