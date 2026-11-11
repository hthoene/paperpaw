//! What the store asks the host about the device: the name plugin libraries call
//! it, the file a code plugin's build is kept in, the kernel's SDK, and what is
//! installed of each plugin the library lists (the interface-7 form of
//! `catalog::Device::of`). Read once for each fetch of a list or a page, held
//! as little as possible (an id and a kind for each installed plugin) and given
//! back when the fetch is over.
//!
//! **Differences from interface 6, on purpose:**
//! - The fingerprint of a build (format 4) is none: interface-7 builds are
//!   dotted (`7.<minor>`), so every library build for the target is looked at.
//! - What the firmware provides (`Provides`: the bytes of its table, the
//!   families and their revisions, the minor) is not known to a plugin, so
//!   every dotted build whose interface is this one fits (`Provides::UNKNOWN`);
//!   the host's own check at the install (`device.md` 5.1, "needs a newer
//!   PaperPaw") has the word, and the store says so from the reason.
//! - The kernel's SDK is the SDK this plugin was built with.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::native::{
    Error, Host,
    device::Device as Dev,
    i18n,
    registry::{PageCursor, Registry, RegistryPluginKind},
    settings::{self, SettingValue},
};
use paperpaw_sdk::{SDK_VERSION, Version};
use paperpaw_ui_kit::{
    mem::{SERVICE_BUFFER, buffer, push},
    text::copy,
};

use crate::native_item::{Device, Have};
use crate::native_model::{FAMILIES, OFFICIAL_LIST, Provides};

/// Registry pages read at most (the plugins installed here).
/// The settings' keys, as the host holds them (the plugin's id and the name).
const CATALOG_KEY: &str = "store.catalog";
const COMMUNITY_KEY: &str = "store.community";
const MOST_PAGES: usize = 16;
/// The flag of a registry row that says the plugin is switched on.
const ENABLED: u32 = 1;

impl Provides {
    /// What the firmware provides, as far as a plugin can tell: nothing that
    /// could make a dotted build of this interface not fit.
    pub const UNKNOWN: Provides = Provides {
        minor: None,
        api: u32::MAX,
        families: [0; FAMILIES],
        oldest: [0; FAMILIES],
        checks_families: false,
    };
}

/// The facts a fetch needs of the device and the settings.
#[derive(Debug, Clone)]
pub struct Facts {
    /// What plugin libraries call this device (`x4`, `x4-pro`, `android`).
    name: String,
    /// The name a build's code gets here, empty where none loads.
    code_file: String,
    /// The user interface language of the request.
    language: String,
    /// Whether the list is PaperPaw's own library.
    official: bool,
    /// What is installed, by id, for the lookups of the list reading.
    have: Vec<(String, Have)>,
}

impl Facts {
    /// Facts as a host or a test states them; `have` is sorted by id here.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(
        name: &str,
        code_file: &str,
        language: &str,
        official: bool,
        mut have: Vec<(String, Have)>,
    ) -> Result<Self, Error> {
        have.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        Ok(Self {
            name: copy(name)?,
            code_file: copy(code_file)?,
            language: copy(language)?,
            official,
            have,
        })
    }

    /// Reads the device and what it has installed; `None` where plugin
    /// libraries do not know this device (a store has nothing made for it).
    ///
    /// # Errors
    /// `NoMemory` and the host's refusals.
    pub fn read(host: &Host<'_>, catalog: &str) -> Result<Option<Self>, Error> {
        let dev = Dev::open(host)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut out = buffer(SERVICE_BUFFER)?;
        let name = {
            let identity = dev.device(host, &mut input, &mut out)?;
            if identity.catalog().is_empty() {
                None
            } else {
                Some(copy(identity.catalog())?)
            }
        };
        let Some(name) = name else {
            dev.release(host).ok();
            return Ok(None);
        };
        // A device with no native build says no code file: no code plugin loads.
        let code_file = match dev.native(host, &mut input, &mut out) {
            Ok(native) => copy(native.code_file())?,
            Err(Error::NotFound | Error::Unsupported) => String::new(),
            Err(error) => {
                dev.release(host).ok();
                return Err(error);
            }
        };
        dev.release(host).ok();
        let mut scratch = [0u8; 16];
        let language = copy(i18n::language(host, &mut scratch)?)?;
        let have = installed(host, &mut input, &mut out)?;
        Ok(Some(Self {
            name,
            code_file,
            language,
            official: catalog.trim() == OFFICIAL_LIST,
            have,
        }))
    }

    /// The language libraries are asked in.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The device's name for the address of a list or a page.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What is installed of `id`.
    #[must_use]
    pub fn have(&self, id: &str) -> Option<Have> {
        self.have
            .binary_search_by(|(known, _)| known.as_str().cmp(id))
            .ok()
            .and_then(|at| self.have.get(at))
            .map(|(_, have)| *have)
    }

    /// The device as the list and page readers take it. `lookup` is
    /// [`Facts::have`] as a closure.
    #[must_use]
    pub fn device<'a>(&'a self, lookup: &'a crate::native_item::HaveFn<'a>) -> Device<'a> {
        Device {
            name: &self.name,
            fingerprint: None,
            code_file: &self.code_file,
            have: Some(lookup),
            sdk: kernel_sdk(),
            official: self.official,
            provides: Some(Provides::UNKNOWN),
        }
    }
}

/// The SDK the data plugins of the library must not be later than.
fn kernel_sdk() -> Version {
    SDK_VERSION
}

/// The plugins installed, sorted by id.
fn installed(
    host: &Host<'_>,
    input: &mut [u8],
    out: &mut [u8],
) -> Result<Vec<(String, Have)>, Error> {
    let registry = Registry::open(host)?;
    let mut have: Vec<(String, Have)> = Vec::new();
    let result = (|| {
        'again: for _ in 0..2 {
            have.clear();
            let mut cursor = PageCursor::start();
            for _ in 0..MOST_PAGES {
                let page = match registry.plugins(host, cursor, input, out) {
                    Ok(page) => page,
                    // The list changed under the reading: start over.
                    Err(Error::State) => continue 'again,
                    Err(error) => return Err(error),
                };
                have.try_reserve(page.len()).map_err(|_| Error::NoMemory)?;
                for plugin in page.rows() {
                    let kind = match plugin.kind() {
                        RegistryPluginKind::Code => Have::Code,
                        RegistryPluginKind::Data => Have::Data,
                        _ => Have::BuiltIn {
                            enabled: plugin.flags() & ENABLED != 0,
                        },
                    };
                    push(&mut have, (copy(plugin.id())?, kind))?;
                }
                match page.end().next_cursor() {
                    Some(next) => cursor = next,
                    None => return Ok(()),
                }
            }
            return Err(Error::NoSpace);
        }
        Err(Error::State)
    })();
    registry.release(host).ok();
    result?;
    have.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Ok(have)
}

/// The catalog address the setting holds.
///
/// # Errors
/// `NoMemory` and the host's refusals.
pub fn catalog_setting(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = buffer(SERVICE_BUFFER)?;
    match settings::read_setting(host, CATALOG_KEY, &mut scratch)? {
        SettingValue::Text(value) => copy(value.trim()),
        _ => copy(OFFICIAL_LIST),
    }
}

/// Whether the reader asked for community plugins too.
///
/// # Errors
/// The host's refusals.
pub fn community(host: &Host<'_>) -> Result<bool, Error> {
    let mut scratch = [0u8; 64];
    Ok(matches!(
        settings::read_setting(host, COMMUNITY_KEY, &mut scratch)?,
        SettingValue::Bool(true)
    ))
}
