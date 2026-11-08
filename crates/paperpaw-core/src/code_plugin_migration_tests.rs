//! Beta 3's `hyphenation-<lang>` code plugins (interface 6) and the data
//! packs of the same ids (interface 7): the update replaces the whole folder
//! through the one install path (`Kernel::place_plugin`, the aside folder and
//! its recovery at start), so Beta 3's `code.ppn` goes only with the pack in
//! place. Tests with a power cut at every write of the swap (todo 200, the
//! kernel's ruling: option b, no second journal).

use super::*;
use alloc::rc::Rc;
use alloc::string::ToString;
use core::cell::{Cell, RefCell};
use paperpaw_platform::{DirEntry, StorageError};
use paperpaw_sdk::MemoryStorage;

const OLD_INI: &str =
    "[plugin]\nid = hyphenation-de\nname = Hyphenation: German\nversion = 1.0.4\n";
const PACK_INI: &str = "[plugin]\nid = hyphenation-de\nname = Hyphenation: German\nversion = 2.0.0\nformats = 5\n\n[hyphenation]\nde = hyphenation/de.pphp\n";
const CODE: &str = "PPNP-beta3-interface-6";
const PART: &str = "plugins/.hyphenation-de.part";

fn table() -> Vec<u8> {
    paperpaw_sdk::hyphen::compile("1b", "", 1, 1)
}

/// A card where the power goes after `left` more changes: from then on every
/// change is refused and nothing is changed. A folder is removed file by
/// file, as the card does, so a cut can leave one half removed.
struct Cut {
    card: Rc<MemoryStorage>,
    left: Cell<usize>,
    changes: Rc<RefCell<Vec<String>>>,
}

impl Cut {
    fn spend(&self, path: &str) -> Result<(), StorageError> {
        self.changes.borrow_mut().push(path.to_string());
        match self.left.get() {
            0 => Err(StorageError::Io),
            n => {
                self.left.set(n - 1);
                Ok(())
            }
        }
    }

    fn files_below(&self, dir: &str, out: &mut Vec<String>) {
        let mut entries: Vec<(String, bool)> = Vec::new();
        let _ = self
            .card
            .list(dir, &mut |e| entries.push((e.name.to_string(), e.is_dir)));
        for (name, is_dir) in entries {
            let path = format!("{dir}/{name}");
            if is_dir {
                self.files_below(&path, out);
            } else {
                out.push(path);
            }
        }
    }
}

impl Storage for Cut {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        self.card.read_at(path, offset, buf)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.card.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.spend(path)?;
        self.card.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.spend(path)?;
        self.card.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.spend(path)?;
        self.card.remove(path)
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        // One directory entry changes at once: whole or not at all.
        self.spend(from)?;
        self.card.rename(from, to)
    }
    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.spend(path)?;
        self.card.create_dir(path)
    }
    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        let mut files = Vec::new();
        self.files_below(path.trim_end_matches('/'), &mut files);
        for file in files {
            self.remove(&file)?;
        }
        Ok(())
    }
}

/// The card as Beta 3 left it, with the pack waiting in its folder.
fn beta3_card(with_pack: bool) -> Rc<MemoryStorage> {
    let card = MemoryStorage::new()
        .with("plugins/hyphenation-de/plugin.ini", OLD_INI)
        .with("plugins/hyphenation-de/code.ppn", CODE)
        .with("plugins/other/plugin.ini", "[plugin]\nname = Other\n");
    if with_pack {
        let card = card
            .with(&format!("{PART}/plugin.ini"), PACK_INI)
            .with(&format!("{PART}/hyphenation/de.pphp"), table());
        return named_installed(card);
    }
    named_installed(card)
}

/// Beta 3's store named its code as installed.
fn named_installed(card: MemoryStorage) -> Rc<MemoryStorage> {
    assert!(paperpaw_sdk::native::remember_code(
        &card,
        "hyphenation-de",
        &[14; 32]
    ));
    Rc::new(card)
}

/// Beta 3's code is the one installed (named as its store did), and this
/// interface-7 firmware's loader refuses it as built for interface 6.
struct Interface6;
impl CodeLoader for Interface6 {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::OlderInterface { built_for: 6 })
    }
}

/// What a start changed, as the paths named, shared so that what the kernel does
/// after it started counts too.
type Changes = Rc<RefCell<Vec<String>>>;

fn start(card: &Rc<MemoryStorage>, left: usize, staged: bool) -> (Kernel, Changes) {
    let changes = Rc::new(RefCell::new(Vec::new()));
    let mut builder = Kernel::builder(crate::tests::DEVICE)
        .storage(Cut {
            card: card.clone(),
            left: Cell::new(left),
            changes: changes.clone(),
        })
        .code_plugins(Native::new("code.ppn"), Box::new(Interface6));
    if staged {
        builder = builder.install_plugin(Id::new_static("hyphenation-de"), PART);
    }
    let kernel = builder.build().unwrap();
    (kernel, changes)
}

fn file(card: &MemoryStorage, path: &str) -> Option<Vec<u8>> {
    card.get(path)
}

/// What the folder of the plugin is, after a start that put it right: the
/// whole Beta 3 plugin, or the whole pack, and nothing of one in the other.
#[derive(Debug, PartialEq, Eq)]
enum Folder {
    Beta3,
    Pack,
}

fn folder(card: &MemoryStorage, when: &str) -> Folder {
    let ini = file(card, "plugins/hyphenation-de/plugin.ini")
        .unwrap_or_else(|| panic!("{when}: the plugin's folder has no plugin.ini"));
    let code = file(card, "plugins/hyphenation-de/code.ppn");
    let pphp = file(card, "plugins/hyphenation-de/hyphenation/de.pphp");
    if ini == OLD_INI.as_bytes() {
        assert_eq!(
            code.as_deref(),
            Some(CODE.as_bytes()),
            "{when}: Beta 3's plugin lost its code"
        );
        assert!(pphp.is_none(), "{when}: Beta 3's folder holds a table");
        Folder::Beta3
    } else {
        assert_eq!(
            ini,
            PACK_INI.as_bytes(),
            "{when}: neither version's plugin.ini"
        );
        assert_eq!(pphp, Some(table()), "{when}: the pack lacks its table");
        assert!(code.is_none(), "{when}: the pack still has Beta 3's code");
        Folder::Pack
    }
}

#[test]
fn a_beta3_hyphenation_plugin_without_a_pack_is_left_alone_and_needs_an_update() {
    let card = beta3_card(false);
    let before = format!("{:?}", snapshot(&card));
    let (kernel, changes) = start(&card, usize::MAX, false);
    let plugin = kernel.plugins().get("hyphenation-de").unwrap();
    assert_eq!(plugin.state, PluginState::Failed);
    assert_eq!(
        plugin.problem,
        Some(PluginProblem::OlderInterface { built_for: 6 })
    );
    // Nothing deleted and nothing written for it.
    assert!(
        !changes
            .borrow()
            .iter()
            .any(|p| p.contains("hyphenation-de")),
        "{:?}",
        changes.borrow()
    );
    assert_eq!(before, format!("{:?}", snapshot(&card)));
    assert_eq!(folder(&card, "no pack"), Folder::Beta3);
    // It stays so at every later start.
    drop(kernel);
    let (kernel, changes) = start(&card, usize::MAX, false);
    assert!(
        !changes
            .borrow()
            .iter()
            .any(|p| p.contains("hyphenation-de")),
        "{:?}",
        changes.borrow()
    );
    assert_eq!(
        kernel.plugins().get("hyphenation-de").unwrap().problem,
        Some(PluginProblem::OlderInterface { built_for: 6 })
    );
}

fn snapshot(card: &MemoryStorage) -> Vec<(String, Vec<u8>)> {
    let mut all = Vec::new();
    for path in [
        "plugins/hyphenation-de/plugin.ini",
        "plugins/hyphenation-de/code.ppn",
        "plugins/other/plugin.ini",
        ".paperpaw/plugin-code.txt",
    ] {
        if let Some(data) = card.get(path) {
            all.push((path.to_string(), data));
        }
    }
    all
}

#[test]
fn the_pack_from_the_bundle_replaces_beta3_and_a_power_cut_anywhere_loses_nothing() {
    // A start that is not cut: how many changes the swap makes.
    let card = beta3_card(true);
    let (kernel, changed) = start(&card, usize::MAX, true);
    let total = changed.borrow().len();
    assert!(total >= 3, "{:?}", changed.borrow());
    assert_eq!(folder(&card, "uncut"), Folder::Pack);
    let plugin = kernel.plugins().get("hyphenation-de").unwrap();
    assert_eq!(plugin.manifest.version, paperpaw_sdk::Version::new(2, 0, 0));
    assert_eq!(plugin.state, PluginState::Running);
    for gone in [PART, "plugins/.hyphenation-de.old"] {
        assert!(
            card.get(&format!("{gone}/plugin.ini")).is_none(),
            "{gone} is left"
        );
    }
    drop(kernel);

    let mut seen = [0usize; 2];
    for cut in 0..total {
        let when = format!("cut after {cut} of {total} changes");
        let card = beta3_card(true);
        // The power goes.
        let (kernel, _) = start(&card, cut, true);
        drop(kernel);
        // The next start, without the bundle's help: the folder is whole.
        let (kernel, _) = start(&card, usize::MAX, false);
        let state = folder(&card, &when);
        seen[usize::from(state == Folder::Pack)] += 1;
        // Beta 3's code is never gone before the pack is.
        if state == Folder::Beta3 {
            assert_eq!(
                kernel.plugins().get("hyphenation-de").unwrap().problem,
                Some(PluginProblem::OlderInterface { built_for: 6 }),
                "{when}"
            );
        }
        drop(kernel);
        // And the bundle's pack, still waiting where it was not taken,
        // finishes the job at the start after.
        let waiting = card.get(&format!("{PART}/plugin.ini")).is_some();
        let (kernel, _) = start(&card, usize::MAX, waiting);
        assert_eq!(folder(&card, &when), Folder::Pack, "{when}");
        assert_eq!(
            kernel.plugins().get("hyphenation-de").unwrap().state,
            PluginState::Running,
            "{when}"
        );
        for gone in ["plugins/.hyphenation-de.old", PART] {
            assert!(
                card.get(&format!("{gone}/plugin.ini")).is_none(),
                "{when}: {gone} is left"
            );
        }
    }
    // The cuts did cut: some left Beta 3's plugin whole, some the pack's.
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

#[test]
fn a_pack_installed_later_from_the_store_replaces_beta3_whole_or_not_at_all() {
    // A start and an install that are not cut: the changes the start makes
    // and the changes the install makes after it.
    let card = beta3_card(true);
    let (mut kernel, changes) = start(&card, usize::MAX, false);
    let at_start = changes.borrow().len();
    kernel.install_plugin(&Id::new_static("hyphenation-de"), PART);
    let install = changes.borrow().len() - at_start;
    assert!(install >= 3, "{:?}", changes.borrow());
    assert_eq!(folder(&card, "uncut"), Folder::Pack);
    drop(kernel);

    let mut seen = [0usize; 2];
    for cut in 0..install {
        let when = format!("store install cut after {cut} of {install} changes");
        let card = beta3_card(true);
        // The power goes after the start's own changes and `cut` of the install's.
        let (mut kernel, _) = start(&card, at_start + cut, false);
        kernel.install_plugin(&Id::new_static("hyphenation-de"), PART);
        drop(kernel);
        // The power came back: the folder is whole, one version or the other.
        let (kernel, _) = start(&card, usize::MAX, false);
        let state = folder(&card, &when);
        seen[usize::from(state == Folder::Pack)] += 1;
        drop(kernel);
        if card.get(&format!("{PART}/plugin.ini")).is_some() {
            // The pack was not taken: the store tries again and gets there.
            let (mut kernel, _) = start(&card, usize::MAX, false);
            kernel.install_plugin(&Id::new_static("hyphenation-de"), PART);
            assert_eq!(folder(&card, &when), Folder::Pack, "{when}");
        } else {
            assert_eq!(
                state,
                Folder::Pack,
                "{when}: the pack was taken but is not in place"
            );
        }
    }
    // The cuts did cut: some left Beta 3's plugin whole, some the pack's.
    assert!(seen[0] > 0 && seen[1] > 0, "{seen:?}");
}

/// Beta 3's store named `hyphenation-de`'s code as installed
/// (`.paperpaw/plugin-code.txt`); the swap leaves that line. It decides
/// nothing for a plugin without code: the start asks it only of the folders
/// that have a code file, so the pack starts as a data plugin with no
/// warning, and a code plugin installed under the id later names its own
/// digest over it.
#[test]
fn the_line_beta3_left_for_the_code_changes_nothing_once_the_pack_is_in_place() {
    let card = beta3_card(true);
    let stale = paperpaw_sdk::native::installed_code(card.as_ref(), "hyphenation-de");
    assert_eq!(stale, Some([14; 32]));
    let (mut kernel, _) = start(&card, usize::MAX, false);
    kernel.install_plugin(&Id::new_static("hyphenation-de"), PART);
    drop(kernel);
    assert_eq!(folder(&card, "installed"), Folder::Pack);
    // Still named: no installer removes it.
    assert_eq!(
        paperpaw_sdk::native::installed_code(card.as_ref(), "hyphenation-de"),
        stale
    );
    // Starts again, as a plugin with no code, running, and without a word
    // said about code that was never installed or changed.
    let (mut kernel, changes) = start(&card, usize::MAX, false);
    let plugin = kernel.plugins().get("hyphenation-de").unwrap();
    assert_eq!(plugin.kind, PluginKind::Data);
    assert_eq!((plugin.state, plugin.problem), (PluginState::Running, None));
    assert!(
        !changes.borrow().iter().any(|p| p.contains("plugin-code")),
        "{:?}",
        changes.borrow()
    );
    assert!(kernel.take_warnings().is_empty());
    // A code plugin put under the id later is named by its own digest, which
    // takes the line's place: the old one grants nothing to it.
    assert!(paperpaw_sdk::native::remember_code(
        card.as_ref(),
        "hyphenation-de",
        &[15; 32]
    ));
    assert_eq!(
        paperpaw_sdk::native::installed_code(card.as_ref(), "hyphenation-de"),
        Some([15; 32])
    );
}
