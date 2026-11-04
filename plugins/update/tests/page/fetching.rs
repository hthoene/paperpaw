//! The plugins fetched for an update: what is told when they did not come
//! along, and what is fetched again at a later check.

use super::*;

#[test]
fn an_update_with_a_plugin_failing_says_it_once_and_plainly() {
    let storage = Shared::default();
    let mut device = with_code(storage, "0.4.0");
    fetch_begun(&mut device);
    let call = device.request();
    device.answer(&call, 0, b"no network");
    // One message: the update is ready, which plugin did not come along,
    // and where to get it.
    let said = device.said.borrow().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert_eq!(
        said[0],
        "Update downloaded. One plugin did not come along. \
         It is fetched again at the next check."
    );
    // Nothing of the fetch is kept once it is over.
    // Except that the plugin is to be fetched again.
    assert!(device.storage.exists(RETRY));
    for name in [
        "fetch.txt",
        "fetch-current.txt",
        "fetch-failed.txt",
        "fetch-native.txt",
    ] {
        assert!(
            !device
                .storage
                .exists(&format!(".paperpaw/plugins/update/{name}")),
            "{name}"
        );
    }
}

#[test]
fn a_fetch_whose_update_is_gone_from_the_card_is_let_go() {
    let storage = Shared::default();
    // An earlier download's fetch, cut off by a restart, with names that
    // failed; its update file is no longer on the card.
    storage
        .write(
            QUEUE,
            b"0.5.0\nbuild stats 0.5.0 - Statistics\n\
              file 120 sha1:0123456789012345678901234567890123456789 plugin.ini https://p.org/stats/plugin.ini\n",
        )
        .unwrap();
    storage.write(FAILED, b"weather").unwrap();
    storage.write(RETRY, b"0.5.0\n1 weather\n").unwrap();
    let mut device = with_code(storage, "0.4.0");
    assert!(!device.storage.exists(QUEUE));
    assert!(!device.storage.exists(FAILED));
    assert!(!device.storage.exists(RETRY));
    // The page shows without fetching anything, nor asking.
    device.run(OPEN);
    device.no_request();
    assert!(device.said.borrow().is_empty());
}

#[test]
fn many_plugins_failing_are_said_as_a_count() {
    let storage = Shared::default();
    let mut device = with_code(storage.clone(), "0.4.0");
    fetch_begun(&mut device);
    // A restart cuts the download off; five had failed before it.
    storage.write(FAILED, b"a\nb\nc\nd\ne").unwrap();
    let mut device = with_code(storage, "0.4.0");
    let call = device.request();
    device.answer(&call, 0, b"no network");
    let said = device.said.borrow().clone();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("6 plugins did not come along."),
        "{said:?}"
    );
}

/// The page shows again: the server is asked, and its answer's builds are
/// fetched; `ok` answers the plugin's files.
fn fetch_again(device: &mut Device, ok: bool) {
    // The page was left and is opened again.
    device.run(actions::BACK);
    device.run(OPEN);
    let call = device.request();
    assert!(call.url.ends_with("/api/v1/updates"), "{}", call.url);
    let code = code_file();
    device.answer(&call, 200, offer_with_code(1000, &code).as_bytes());
    let call = device.request();
    assert_eq!(call.url, "https://p.org/stats/plugin.ini");
    if ok {
        device.answer(&call, 200, PLUGIN_INI.as_bytes());
        let call = device.request();
        device.answer(&call, 200, &code);
    } else {
        device.answer(&call, 0, b"no network");
    }
}

#[test]
fn a_plugin_that_failed_is_fetched_again_when_the_page_shows() {
    let storage = Shared::default();
    let mut device = with_code(storage, "0.4.0");
    fetch_begun(&mut device);
    let call = device.request();
    device.answer(&call, 0, b"no network");
    assert!(device.storage.exists(RETRY));
    fetch_again(&mut device, true);
    assert!(device.storage.exists("plugins/.stats.next/code.ppn"));
    assert!(!device.storage.exists(RETRY));
    // Said once, when the update was downloaded; the fetch again is quiet.
    assert_eq!(device.said.borrow().len(), 1, "{:?}", device.said.borrow());
    assert!(device.shown().contains("Install"));
    // Nothing to fetch again: the page shows without asking.
    device.run(actions::BACK);
    device.run(OPEN);
    device.no_request();
}

#[test]
fn a_plugin_failing_again_and_again_is_left_to_the_store() {
    let storage = Shared::default();
    let mut device = with_code(storage, "0.4.0");
    fetch_begun(&mut device);
    let call = device.request();
    device.answer(&call, 0, b"no network");
    // Two more tries, then no more: the third says so.
    fetch_again(&mut device, false);
    assert_eq!(device.said.borrow().len(), 1);
    fetch_again(&mut device, false);
    let said = device.said.borrow().clone();
    assert_eq!(said.len(), 2, "{said:?}");
    assert_eq!(
        said[1],
        "One plugin could not be fetched after three tries. Get it from the store."
    );
    assert!(!device.storage.exists(RETRY));
    device.run(actions::BACK);
    device.run(OPEN);
    device.no_request();
}

#[test]
fn plugins_to_fetch_again_are_let_go_when_the_update_is_installed() {
    let storage = Shared::default();
    let mut device = with_code(storage.clone(), "0.4.0");
    fetch_begun(&mut device);
    let call = device.request();
    device.answer(&call, 0, b"no network");
    assert!(device.storage.exists(RETRY));
    // Installed from elsewhere: started as 0.5.0.
    let device = with_code(storage, "0.5.0");
    assert!(!device.storage.exists(RETRY));
}
