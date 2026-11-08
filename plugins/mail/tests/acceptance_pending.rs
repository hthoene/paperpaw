//! What the interface-6 Mail does that the native one cannot yet, each an ignored
//! case that names the host piece it waits for (see `PORTING-7.md`).

#[test]
#[ignore = "needs the sockets host and facade (docs/abi/sockets.md is a draft): connect, TLS, STARTTLS, pinned keys"]
fn the_folders_and_the_mails_of_an_account_are_read_over_imap() {
    unreachable!("login, LIST, STATUS, SEARCH, FETCH of headers and bodies in pieces");
}

#[test]
#[ignore = "needs the sockets host and the secrets facade (#367 is open): the password is read, used and wiped"]
fn a_mail_is_sent_over_smtp_with_the_accounts_password() {
    unreachable!("AUTH PLAIN or LOGIN after the TLS handshake; the password wiped after use");
}

#[test]
#[ignore = "needs the work service (docs/abi/work.md): the connection runs in the background and ends with the screen"]
fn a_connection_ends_with_the_screen_that_opened_it() {
    unreachable!("one socket per kind of screen, closed when the screen closes");
}

#[test]
#[ignore = "needs the settings facade for writes and the SettingChanged event: choosing a provider fills in the servers"]
fn choosing_a_provider_or_typing_a_known_address_fills_in_the_servers() {
    unreachable!("provider::commands writes the server, port and security settings");
}

#[test]
#[ignore = "needs core.ui compose (screens) over core.files and the card root: the folders, the list, the reader, the compose window"]
fn the_screens_read_and_write_mail_from_the_cards_cache() {
    unreachable!("boxes, list, read and compose; the store keeps what was fetched on the card");
}

#[test]
#[ignore = "needs the widget host call (Core todo 76): unread, latest, starred, mailbox and write widgets"]
fn the_home_widgets_show_the_mail_checked_last() {
    unreachable!("they read the store, never the network");
}

#[test]
#[ignore = "needs the pinned-key question (Question::choice) and the sockets certificate event"]
fn an_unknown_servers_key_is_offered_once_and_a_changed_one_never() {
    unreachable!("account::offer: Ask, Changed, Attack");
}
