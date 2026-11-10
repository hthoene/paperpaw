### Added

- **A tap forwarded to a widget reaches its owner** (#228). `core.ui` op 6 queued taps and long presses that nothing delivered, so it answered unsupported. The kernel now delivers them between callbacks, a few a pass, to the owner's widget `input` callback with its command stage, applies the guard rule again at the time of delivery, and never delivers to a dropped instance or an owner that cannot be called. Op 6 is served from the first pass on. The widget-drop events were already delivered.
