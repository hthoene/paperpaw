//! The kernel's side of Bluetooth (see [ADR 0027](../../../docs/adr/0027-bluetooth.md)
//! and [`io::bluetooth`]): plugins' commands checked and passed to the
//! radio's state, the shell's reports turned into plugins' events and
//! into keys, bonds kept with the secrets, and pairing asked of the
//! reader.

#[allow(
    unused_imports,
    reason = "a part of the kernel, which sees all of it; not every part uses every name"
)]
use super::*;

use paperpaw_sdk::bluetooth::{BtError, BtEvent, BtFailure, BtOp, BtRequest, DeviceId};
use paperpaw_sdk::event::{Event, EventData};
use paperpaw_sdk::replies::Confirmed;
use paperpaw_sdk::{Arg, Uses};

use crate::io::bluetooth::{Asked, BOND_PREFIX, BtReport, PairQuestion};

/// The reader's answer to a pairing's question.
const PAIR_ANSWERED: Event<Confirmed> = Event::new("core.bluetooth-pair-answered");
/// The device a pairing question is about, in its context.
const DEVICE: &str = "device";
/// A number the answer must bring back: only the question's own counts.
const NONCE: &str = "nonce";

impl Kernel {
    /// Bluetooth as plugins see it (the radio, the bonded devices), for a
    /// shell that tells its own host services of it (`core.device`, ops 10
    /// and 11). `None` where the kernel is built without Bluetooth.
    pub fn bluetooth_model(&self) -> Option<&paperpaw_sdk::bluetooth::Bluetooth> {
        self.shared.bluetooth.as_ref()
    }

    /// Plugin `request.from()`'s Bluetooth command, answered on its events
    /// where it fails.
    pub(crate) fn apply_bluetooth(&mut self, request: BtRequest) {
        let (command, from) = request.into_parts();
        let plugin = from.as_ref().map_or("", |f| f.as_str()).to_string();
        let asked = Asked {
            screens: self.stack.len(),
            reader: self.shared.reader,
            now: self.now,
        };
        let allowed = plugin == crate::meter::CORE
            || self
                .shared
                .plugins
                .get(&plugin)
                .is_some_and(|info| info.manifest.uses.contains(Uses::BLUETOOTH));
        let answer = self
            .io
            .bluetooth
            .as_ref()
            .and_then(|radio| radio.answer_to(&plugin, &command));
        let result = match &mut self.io.bluetooth {
            None => Err(BtFailure::new(BtOp::Extended, BtError::Off)),
            Some(_) if !allowed => Err(BtFailure::new(BtOp::Extended, BtError::NotAllowed)),
            Some(radio) => radio.command(&plugin, command, asked, &mut self.output.commands),
        };
        if let Err(failure) = result {
            match answer {
                Some(events) => {
                    let event = BtEvent::Failed(failure);
                    self.output
                        .commands
                        .push(Command::Emit(events, event.into_params()));
                }
                None => self.warn_runtime(format!(
                    "{plugin}: Bluetooth {}: {}",
                    failure.op.as_str(),
                    failure.error
                )),
            }
        }
        self.sync_bluetooth();
    }

    /// What the shell reports about Bluetooth (see
    /// [`KernelBuilder::bluetooth`]): plugins hear of it, a remote's keys
    /// go to the key maps.
    pub fn bluetooth_event(&mut self, now: Instant, report: BtReport) -> Effects {
        let running = self.shared.meter.enter(meter::CORE);
        self.now = now;
        let keys = match &mut self.io.bluetooth {
            Some(radio) => {
                radio.report(report, now, &mut self.output.commands);
                radio.take_keys()
            }
            None => Vec::new(),
        };
        self.sync_bluetooth();
        drop(running);
        let mut effects = self.finish();
        for (name, pressed) in keys {
            let state = if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            };
            effects = effects.and(self.handle(now, InputEvent::Button { name, state }));
        }
        effects
    }

    /// When Bluetooth waits for a time.
    pub(crate) fn bluetooth_deadline(&self) -> Option<Instant> {
        self.io.bluetooth.as_ref().and_then(|r| r.deadline())
    }

    /// Time passed for Bluetooth: scans and advertising that ended, the
    /// radio off.
    pub(crate) fn bluetooth_tick(&mut self) {
        if let Some(radio) = &mut self.io.bluetooth {
            radio.tick(self.now, &mut self.output.commands);
            self.sync_bluetooth();
        }
    }

    /// Fewer screens are open: what they asked of Bluetooth ends.
    pub(crate) fn bluetooth_screens_closed(&mut self) {
        let screens = self.stack.len();
        if let Some(radio) = &mut self.io.bluetooth {
            radio.close_for_closed_screens(screens, self.now, &mut self.output.commands);
            self.sync_bluetooth();
        }
    }

    /// The device goes to sleep: Bluetooth ends.
    pub(crate) fn bluetooth_suspend(&mut self) {
        if let Some(radio) = &mut self.io.bluetooth {
            radio.suspend(self.now, &mut self.output.commands);
            self.sync_bluetooth();
        }
    }

    /// An answer to the pairing question, where `event` is one.
    // Its own frame: not inlined into `emit_from` (#250).
    #[inline(never)]
    pub(crate) fn bluetooth_answer(&mut self, event: &Id, params: &Params) -> bool {
        let Some(answer) = PAIR_ANSWERED.read(event, params) else {
            return false;
        };
        let context = &answer.context;
        let device = context.get(DEVICE).and_then(|d| d.parse().ok());
        let nonce = context.get(NONCE).and_then(|n| n.parse::<u64>().ok());
        // Only the question's own answer, once.
        if let (Some(device), Some(nonce), Some(radio)) = (device, nonce, &mut self.io.bluetooth)
            && Some(nonce) == self.pairing_nonce
        {
            self.pairing_nonce = None;
            radio.answer(DeviceId::new(device), answer.yes);
        }
        true
    }

    /// What every plugin sees, the bonds kept, a pairing question asked.
    pub(crate) fn sync_bluetooth(&mut self) {
        // The Bluetooth manager: the plugin whose screen the slot shows.
        let manager = self
            .shared
            .registry
            .resolve_slot(paperpaw_sdk::builtin::slots::BLUETOOTH.as_str())
            .map(|screen| String::from(screen.namespace()));
        let Some(radio) = &mut self.io.bluetooth else {
            return;
        };
        radio.set_manager(manager);
        self.shared.usage.bluetooth_ms = self
            .shared
            .usage
            .bluetooth_ms
            .saturating_add(radio.take_on_ms(self.now));
        let state = radio.state();
        if self.shared.bluetooth.as_ref() != Some(&state) {
            self.shared.bluetooth = Some(state);
            self.output
                .commands
                .push(events::BLUETOOTH_CHANGED.emit(()));
        }
        let kept: Vec<(String, String)> = self
            .shared
            .secrets
            .iter()
            .filter(|s| s.id.starts_with(BOND_PREFIX))
            .map(|s| (s.id.clone(), s.value.clone()))
            .collect();
        let bonds = radio.take_bonds(&kept);
        let question = radio.take_question();
        let ended = radio.take_pairing_ended();
        for (id, label, value) in bonds {
            let change = if value.is_empty() {
                paperpaw_sdk::secrets::SecretChange::remove(&id)
            } else {
                paperpaw_sdk::secrets::SecretChange::keep(&id, &label, &value).changeable(false)
            };
            self.set_secret(change);
        }
        if let Some((_, lost, name)) = ended {
            self.end_pairing(lost, &name);
        }
        if let Some((device, question, name)) = question {
            self.ask_pairing(device, question, &name);
        }
    }

    /// The pairing question closes where it shows: the pairing is over,
    /// and where its link was lost (`lost`) a notice says so.
    fn end_pairing(&mut self, lost: bool, name: &str) {
        let Some(nonce) = self.pairing_nonce.take() else {
            return;
        };
        let nonce = nonce.to_string();
        let open = self
            .stack
            .iter()
            .find(|s| s.params.get(NONCE) == Some(nonce.as_str()))
            .map(|s| s.serial);
        if let Some(serial) = open {
            self.output
                .commands
                .push(Command::Close(paperpaw_sdk::ScreenKey::new(serial)));
        }
        if lost {
            let i18n = &self.shared.translator;
            let name = if name.is_empty() {
                i18n.tr("core.bluetooth.unnamed").to_string()
            } else {
                String::from(name)
            };
            let text = i18n.format("core.bluetooth.pair-lost", &[("name", Arg::Str(&name))]);
            self.output.commands.push(Command::Notify(text));
        }
    }

    /// Asks the reader whether to pair with `device`, in the kernel's own
    /// words.
    fn ask_pairing(&mut self, device: DeviceId, question: PairQuestion, name: &str) {
        let mut bytes = [0u8; 8];
        self.shared.random.fill(&mut bytes);
        let nonce = u64::from_le_bytes(bytes) ^ self.now.0;
        self.pairing_nonce = Some(nonce);
        let i18n = &self.shared.translator;
        let name = if name.is_empty() {
            i18n.tr("core.bluetooth.unnamed").to_string()
        } else {
            String::from(name)
        };
        let title = i18n.format("core.bluetooth.pair", &[("name", Arg::Str(&name))]);
        let text = match question {
            PairQuestion::Confirm => String::new(),
            PairQuestion::Compare(code) => i18n.format(
                "core.bluetooth.compare",
                &[("code", Arg::Str(&format!("{code:06}")))],
            ),
            PairQuestion::Show(code) => i18n.format(
                "core.bluetooth.type",
                &[("code", Arg::Str(&format!("{code:06}")))],
            ),
        };
        let context = Params::new()
            .with(DEVICE, device.number().to_string())
            .with(NONCE, nonce.to_string());
        let ask = paperpaw_sdk::ask::confirm(title, &PAIR_ANSWERED)
            .text(text)
            .yes(i18n.tr("core.bluetooth.pair.yes"))
            .no(i18n.tr("core.bluetooth.pair.no"))
            .context(&context)
            .command();
        self.output.commands.push(ask);
    }
}
