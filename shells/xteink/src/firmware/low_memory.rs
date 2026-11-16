//! The one-time low-memory notice (#248): the main loop's side of
//! [`paperpaw_x4::low_memory`], whose rule decides everything: the heap is
//! looked at in one state only (Home on top, no radio on, the start settled,
//! store plugins running), once for each set of plugins, and a short block
//! asks the kernel to show the notice over Home. Nothing polls: it runs where
//! the kernel's work was applied. What is kept is a few bytes of state and,
//! once the notice was shown, a file of eight digits on the card.

use log::info;
use paperpaw_sdk::{PluginKind, PluginState};
use paperpaw_x4::low_memory::{self, Facts};

use super::Firmware;
use crate::now;

/// Where the set it was shown for is kept.
const FILE: &str = paperpaw_sdk::data_path!("low-memory.txt");

impl Firmware {
    /// The notice's effects where it was shown now. `busy`: this call's
    /// effects ask to sleep, switch off or restart, or showed grays (the
    /// notice would wipe them: it comes with the next call).
    pub(super) fn low_memory_notice(&mut self, busy: bool) -> Option<paperpaw_core::Effects> {
        let mut store = false;
        let set = low_memory::fingerprint(self.kernel.plugins().iter().filter_map(|plugin| {
            let running = plugin.state == PluginState::Running;
            store |= running && plugin.kind == PluginKind::Code;
            running.then_some(plugin.manifest.id.as_str())
        }));
        let busy = busy || self.state.sleep || self.state.power_off;
        let facts = Facts {
            store_plugins: store,
            set,
            on_home: self.kernel.home_on_top(),
            radio_on: self.network.radio_on(),
            up_ms: now().0,
            busy,
        };
        if self.state.low_memory.needs_card(store) {
            let kept = self.read_low_memory_file();
            self.state.low_memory.kept(kept);
        }
        let shown = self.state.low_memory.step(&facts, || {
            let largest = crate::largest_block();
            info!("low memory: largest block {largest} B on Home, store plugins running");
            largest
        })?;
        let (opened, effects) = self.kernel.show_low_memory_notice(now());
        if !opened {
            return None;
        }
        self.state.low_memory.showed(shown);
        let storage = self.kernel.storage();
        let _ = storage.create_dir(paperpaw_sdk::data::DIR);
        let _ = storage.write(FILE, &low_memory::render(shown));
        info!("low memory: the notice is shown");
        Some(effects)
    }

    /// What the card kept.
    fn read_low_memory_file(&self) -> Option<u32> {
        let mut digits = [0u8; 8];
        let read = self.kernel.storage().read_at(FILE, 0, &mut digits).ok()?;
        low_memory::parse(core::str::from_utf8(digits.get(..read)?).ok()?)
    }
}
