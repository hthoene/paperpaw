### Added

- Interface 7 (additive, `since` 1): `command.show_again` (1598; SDK `CommandSender::show_again`, `cancel_show_again`) lets the sleep screen ask to be shown again while the device sleeps, once or every `every_ms` (at least a minute), and `dev_device.caps` bit 15 `refresh_asleep` (SDK `DeviceCapability::RefreshAsleep`; kernel `Capability::REFRESH_ASLEEP`) says where that works: the X4 and X3 draw the screen again from their light sleep's wake (a partial refresh, never earlier than 5 minutes after the last, never at 10 % battery or less, no radio); the X4 Pro and Classic, and the emulator's other devices, do not. The emulator's X4 and X3 profiles do it on their timer. The sleep plugin's *Redraw while asleep* (off, 15, 30 or 60 minutes; off by default) uses it to keep a clock, calendar or weather widget right, with the cost named in its help (about 0.03 mAh a redraw, an estimate) (#319).
- Interface 7 (additive): input-filter verdict 6 `preview` (`core.input-filter@1` revision 1, optional `key` and `value` of `filter.verdict`; SDK `settings::encode_value`) takes the input and shows a setting's value without a card write until the capture ends; a filter keeps it by answering the up with `run`. The Edge light plugin uses it for a stepless edge swipe: the light follows the finger and one setting write happens when it lifts (#104).

### Changed

- The `filter.verdict` record of `core.input-filter` gained two appended fields, so `FilterVerdict::new` takes `key` and `value` (empty for the older verdicts).
