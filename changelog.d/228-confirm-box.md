### Changed
- The host's two confirmations, a question of kind 2 and the firmware confirmation, now draw, scroll and move their focus through one shared box (`dialogs::ConfirmBox`); what their buttons mean stays with each screen. The firmware confirmation's focus changes redraw only the buttons concerned, as the question's do.
