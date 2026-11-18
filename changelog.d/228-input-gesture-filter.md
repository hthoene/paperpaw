### Added

- **Native input filters see the gestures the kernel recognizes** (#228). A tap, long press or swipe goes to the filters of `core.input-filter` as an `input.gesture` record before the screens: a filter may pass it, replace it by another gesture, take or cancel it, run an action, or raise an input of its plugin's. A gesture a filter made, and an input a filter raised, do not go through the filters again. A filter whose call fails at three inputs in a row is left out (and logged) until the declarations change, instead of being called and the list read again at every key.
