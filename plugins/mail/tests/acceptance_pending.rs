//! What the interface-6 Mail does that the native one cannot yet, each an ignored
//! case that names the host piece it waits for (see `PORTING-7.md`).

#[test]
#[ignore = "needs a host that runs native plugins end to end with core.sockets and the screens that start a connection: the session, the link and the job are tested on a socket double (tests/native_session.rs, native_link.rs, native_job.rs)"]
fn the_folders_and_the_mails_of_an_account_are_read_over_imap() {
    unreachable!("login, LIST, STATUS, SEARCH, FETCH of headers and bodies in pieces");
}

#[test]
#[ignore = "needs a host that runs native plugins end to end with core.sockets and core.secrets: the password is read from the secrets, used and wiped (the endpoint and the wipes are tested in tests/native_account.rs and native_session.rs)"]
fn a_mail_is_sent_over_smtp_with_the_accounts_password() {
    unreachable!("AUTH PLAIN or LOGIN after the TLS handshake; the password wiped after use");
}

#[test]
#[ignore = "needs a scripted host with core.ui, core.sockets and storage (#944): the list screens (native_screens.rs) hold the one connection and close it when they close or another screen opens above; the job closing its socket on a host error is tested in tests/native_job.rs"]
fn a_connection_ends_with_the_screen_that_opened_it() {
    unreachable!("one socket per kind of screen, closed when the screen closes");
}

#[test]
#[ignore = "needs the settings facade for writes and the SettingChanged event: choosing a provider fills in the servers"]
fn choosing_a_provider_or_typing_a_known_address_fills_in_the_servers() {
    unreachable!("provider::commands writes the server, port and security settings");
}

#[test]
#[ignore = "needs a scripted host with core.ui, storage and sockets (#944): the folders, the list, the reader and the compose window are native, and what they do with the host is not run end to end"]
fn the_screens_read_and_write_mail_from_the_cards_cache() {
    unreachable!("boxes, list, read and compose; the store keeps what was fetched on the card");
}

#[test]
#[ignore = "needs a scripted host with the widget stage (#944): unread, latest, starred, mailbox and write widgets are native and drawn by `native_widgets`, but no host runs them end to end"]
fn the_home_widgets_show_the_mail_checked_last() {
    unreachable!("they read the store, never the network");
}

#[test]
#[ignore = "needs a scripted host with core.ui (#944): the list screens ask Trust this server? (Question::confirm, Trust and Don't trust) and keep the key with set_setting; the offer itself (account::offer: Ask, Changed, Attack) and the Untrusted run are native and tested"]
fn an_unknown_servers_key_is_offered_once_and_a_changed_one_never() {
    unreachable!("account::offer: Ask, Changed, Attack");
}
