//! The one-time low-memory notice (#248): the main loop's side of
//! [`paperpaw_x4::low_memory`]. After the work of a call has been shown, and
//! once the start has settled, it looks at the heap's largest free block
//! where store plugins are loaded (a search, so only once for each set of
//! plugins) and, where the block is under the rule's limit, asks the kernel
//! to show the notice. The kernel shows it only over the home screen: never
//! over the first-run setup, a book, a dialog or the sleep screen. Nothing
//! polls: it runs where the kernel's work was applied. What is kept is 12
//! bytes of state and a file of eight digits on the card.

use log::info;
use paperpaw_sdk::{PluginKind, PluginState};
use paperpaw_x4::low_memory::{self, Notice};

use super::Firmware;
use crate::now;

/// The start has settled this long after it (covers, indexes and the plugins'
/// first work done): the heap's reading means something from here on.
const SETTLED_MS: u64 = 10_000;

/// Where the set it was shown for is kept.
const FILE: &str = paperpaw_sdk::data_path!("low-memory.txt");

impl Firmware {
    /// The notice's effects where it was shown now.
    pub(super) fn low_memory_notice(&mut self) -> Option<paperpaw_core::Effects> {
        if now().0 < SETTLED_MS {
            return None;
        }
        let mut store = false;
        let set = low_memory::fingerprint(self.kernel.plugins().iter().filter_map(|plugin| {
            let running = plugin.state == PluginState::Running;
            store |= running && plugin.kind == PluginKind::Code;
            running.then_some(plugin.manifest.id.as_str())
        }));
        let kept = self.read_low_memory_file();
        let notice = self.state.low_memory.get_or_insert(Notice::new(kept));
        notice.set_changed(set);
        if notice.wants_look(store, set) {
            let largest = crate::largest_block();
            info!("low memory: largest block {largest} B, store plugins loaded");
            notice.looked(set, largest);
        }
        let due = notice.due()?;
        let (shown, effects) = self.kernel.show_low_memory_notice(now());
        if !shown {
            return None;
        }
        if let Some(notice) = self.state.low_memory.as_mut() {
            notice.showed(due);
        }
        let storage = self.kernel.storage();
        let _ = storage.create_dir(paperpaw_sdk::data::DIR);
        let _ = storage.write(FILE, &low_memory::render(due));
        info!("low memory: the notice is shown");
        Some(effects)
    }

    /// What the card kept, read once.
    fn read_low_memory_file(&self) -> Option<u32> {
        if self.state.low_memory.is_some() {
            return None;
        }
        let mut digits = [0u8; 8];
        let read = self.kernel.storage().read_at(FILE, 0, &mut digits).ok()?;
        low_memory::parse(core::str::from_utf8(digits.get(..read)?).ok()?)
    }
}
