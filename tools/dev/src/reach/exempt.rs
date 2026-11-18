//! The screens the searches do not look for (see [`super`]), each with
//! why.

/// Screens the search does not look for, with why: the device shows
/// them when something happens to it, not where a reader goes with the
/// buttons, or the checks' device cannot come by them. Each is a note
/// with its reason; one the buttons lead to after all is a warning (take
/// it off the list).
pub const EXEMPT: &[(&str, &str)] = &[
    (
        "core.notice-box",
        "the kernel's notice: it shows it over whatever is open when something is said \
         (`command.notify`, its own messages) and takes it away after `core.notice-seconds`",
    ),
    (
        "core.low-memory",
        "the kernel shows it once over the home screen where the shell finds the memory \
         short with store plugins loaded: nothing pressed opens it",
    ),
    (
        "core.finishing-update",
        "the shell opens it while a firmware just installed runs on trial, until the restart: \
         nothing pressed opens it, and it takes no input",
    ),
    (
        "core.placeholder",
        "it is bound to the standard slots at the lowest priority and shown only where no \
         plugin gives a slot its screen: in the app every slot has one",
    ),
    (
        "transfer.cable",
        "the screen of the slot `core.cable`: the kernel opens it when a computer at the \
         cable asks for the library, not a press",
    ),
    (
        "store.offer",
        "a page of the first-run setup (a setup step of the store); the checks start with \
         the setup done",
    ),
    (
        "store.templates",
        "a page of the first-run setup (a setup step of the store); the checks start with \
         the setup done",
    ),
    (
        "bluetooth.page",
        "its row in the settings and its action are offered on a device with a Bluetooth \
         radio (`MenuNeed::Bluetooth`), and the emulator has none",
    ),
    (
        "update.page",
        "its row under Settings, Updates is offered on a device whose firmware PaperPaw is \
         (`MenuNeed::Firmware`), and the emulator has no firmware to update",
    ),
    (
        "store.fetch",
        "shown while the store fetches (the list, the plugins a book asks for, Update all) \
         and gone by itself when all is here: with the emulator's fixtures a fetch is over \
         within the press that starts it",
    ),
    (
        "fonts-browser.font",
        "a font of the list of fonts to download, and with the emulator's fixtures that \
         list stays at \"Loading fonts\" (#366)",
    ),
    (
        "widgets.todo.list",
        "nothing opens it yet: a press on the to-do widget does not run its action \
         `widgets.todo.open`, and no menu has it (#364)",
    ),
    (
        "home.drawer",
        "its action `home.all-tools` is bound to a swipe up on the home screen alone: with \
         buttons nothing leads to it unless the reader binds one, and the main menu's Tools \
         lists the same tools (#365)",
    ),
    (
        "bluetooth.search",
        "its plugin asks for a scan first and the emulator has no radio, which refuses it: \
         the screen closes with a notice, as on a device without one",
    ),
];

/// Why `screen` is not looked for, if it is declared so ([`EXEMPT`]).
pub fn exempt(screen: &str) -> Option<&'static str> {
    EXEMPT
        .iter()
        .find(|(exempt, _)| *exempt == screen)
        .map(|(_, why)| *why)
}
