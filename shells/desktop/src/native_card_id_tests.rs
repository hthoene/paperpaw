//! A card folder named like a linked plugin, on an emulated X4 (#1424 R1):
//! its code goes through the loader's [`HostLoader::native`], which refuses
//! the id; the plugin list's row is then the linked plugin's.

use std::rc::Rc;

use paperpaw_core::sdk::{MemoryStorage, PluginKind, PluginState};

use super::*;
use crate::device::Device;

/// The linked plugin whose id the card folder takes.
const ID: &str = "help";
const FOLDER: &str = "plugins/help";
/// A text only the card folder's translations have.
const CARD_KEY: &str = "help.card-folder-only";

/// The emulator's loader with every file taken as an interface-7 library:
/// what refuses the id is [`HostLoader::native`] itself, whatever the bytes.
struct EveryFileNative(HostLoader);

impl CodeLoader for EveryFileNative {
    fn load(
        &mut self,
        _: &dyn Storage,
        _: &CodeFile<'_>,
    ) -> Result<Box<dyn Plugin>, PluginProblem> {
        Err(PluginProblem::Unsupported)
    }

    fn load_native(
        &mut self,
        storage: &dyn Storage,
        code: &CodeFile<'_>,
    ) -> Result<Option<Box<dyn paperpaw_core::NativeAdmission>>, PluginProblem> {
        let bytes = read_installed(storage, &Installed::of(code))?;
        self.0.native(&bytes, code).map(Some)
    }
}

fn add(builder: paperpaw_core::KernelBuilder) -> paperpaw_core::KernelBuilder {
    let loader = HostLoader::for_device(MemoryClass::Small);
    builder.code_plugins(native(), Box::new(EveryFileNative(loader)))
}

/// A folder `help` with code installed for it and a translation of its own.
fn card() -> Rc<MemoryStorage> {
    let code = b"not the help plugin".to_vec();
    let sha256 = paperpaw_core::sdk::digest::sha256::digest(&code);
    let files = MemoryStorage::new()
        .with(
            &format!("{FOLDER}/plugin.ini"),
            "[plugin]\nname = Not Help\nversion = 9.0.0\n",
        )
        .with(&format!("{FOLDER}/{}", file_name()), code)
        .with(
            &format!("{FOLDER}/i18n/en.ini"),
            format!(
                "[meta]\nlanguage = en\nname = English\nplural = one-other\n\n\
                 [messages]\n{CARD_KEY} = from the card\n"
            ),
        );
    assert!(paperpaw_core::sdk::native::remember_code(
        &files, ID, &sha256
    ));
    Rc::new(files)
}

#[test]
fn a_card_folder_with_a_linked_plugins_id_leaves_the_row_to_the_linked_plugin() {
    let _one = crate::one_device_at_a_time();
    assert!(paperpaw_app::is_linked_id(ID));
    let profile = crate::profiles::find("xteink-x4").unwrap();
    let files = card();
    let mut device = Device::in_memory(profile, None, files.clone(), true, Some(add)).unwrap();
    device.start();
    let kernel = &device.kernel;
    let row = kernel
        .plugins()
        .get(ID)
        .expect("the linked plugin is listed");
    assert_eq!(
        row.kind,
        PluginKind::BuiltIn,
        "the row is the linked plugin's"
    );
    assert_eq!(row.problem, None);
    assert_eq!(row.state, PluginState::Running);
    assert!(
        row.manifest.name != "Not Help",
        "not the card folder's manifest"
    );
    // Nothing of the refused folder is loaded: not its translations.
    assert_eq!(kernel.translator().get(CARD_KEY), None);
    // Its refusal stays visible (the device keeps what it was told).
    let warned = device.warnings.iter().any(|warning| {
        matches!(&warning.source, paperpaw_core::WarningSource::Plugin(id) if id.as_str() == ID)
            && warning.message.contains("Mismatch")
    });
    assert!(warned, "the refusal is told");
    // The row's kind is what the plugin manager offers by (linked: no
    // uninstall; off is the linked plugin's own switch); the kernel test
    // `a_folder_refused_for_its_id_gives_its_row_to_the_linked_plugin` shows
    // removing the id never reaches the folder.
    assert!(
        files.size(&format!("{FOLDER}/plugin.ini")).is_ok(),
        "the folder is kept"
    );
}
