//! What an installer keeps for a plugin once it is in place: its code
//! named as installed ([`native::remember_code`]), its author, where it
//! came from ([`origin`](super::origin)). Kept before the kernel installs
//! the plugin, each would be wrong where the install fails or the power
//! is cut first: the version before would be refused at the next start,
//! or lose its store. So the installer writes them into the folder it
//! hands over ([`write()`]), and the kernel keeps them once the folder is
//! the plugin's ([`settle`]): right after the install, or at the next
//! start where that was cut short. The file is hidden: neither the Wi-Fi
//! transfer nor the cable reach it.

use alloc::format;
use alloc::string::String;

use paperpaw_platform::Storage;

use crate::Version;
use crate::native;










#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryStorage;
    use crate::install::{Origin, origin};

    /// Kept once settled, the file gone; a local one forgets the store.
    #[test]
    fn marks_are_kept_once_the_plugin_is_in_place() {
        let storage = MemoryStorage::new();
        let v = Version::new(1, 2, 0);
        assert!(write(&storage, "plugins/dark/", &Marks::new(From::Store(v))).is_ok());
        assert_eq!(origin(&storage, "dark", v), Origin::Local);
        assert!(settle(&storage, "dark", "plugins/dark"));
        assert_eq!(origin(&storage, "dark", v), Origin::Store);
        assert!(!storage.exists("plugins/dark/.install-marks"));
        assert!(!settle(&storage, "dark", "plugins/dark"));
        assert!(write(&storage, "plugins/dark", &Marks::new(From::Local)).is_ok());
        settle(&storage, "dark", "plugins/dark");
        assert_eq!(origin(&storage, "dark", v), Origin::Local);
    }

    /// Code is named only where the folder holds that code: marks put
    /// beside other code name nothing.
    #[test]
    fn code_is_named_only_for_the_code_in_the_folder() {
        let storage = MemoryStorage::new();
        let marks = Marks::new(From::Local).code([7; 32]);
        assert!(write(&storage, "plugins/clock", &marks).is_ok());
        settle(&storage, "clock", "plugins/clock");
        assert_eq!(native::installed_code(&storage, "clock"), None);
    }
}
