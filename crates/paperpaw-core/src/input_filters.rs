//! What the input filters make of an input, before screens, gestures and key
//! maps: the native `core.input-filter` chain first
//! ([`native_host::input_chain`]), then the interface-6 filters.
use super::*;

impl Kernel {
    /// `event` after the native filters and then the interface-6 ones, or the
    /// effects of an input one of them took, cancelled or ran an action for.
    pub(crate) fn filtered(&mut self, now: Instant, event: InputEvent) -> Result<InputEvent, Effects> {
        let event = self.native_filtered(now, event)?;
        self.legacy_filtered(now, event)
    }

    /// The interface-6 filters: while a screen guards the device, or a PIN or
    /// a password is entered, only those of its plugin.
    fn legacy_filtered(&mut self, now: Instant, event: InputEvent) -> Result<InputEvent, Effects> {
        let (verdict, delivered) = {
            let asking = self.secret_owner();
            let env = self.shared.env(now);
            self.shared
                .input_filters
                .filter_of(&env, &event, asking.as_deref())
        };
        let event = match verdict {
            InputVerdict::Take => {
                // Taken from the recognizers that follow the finger too.
                if let InputEvent::Pointer(p) = delivered {
                    self.recognizers_lose(&p);
                }
                return Err(self.finish());
            }
            InputVerdict::Run(action) => {
                self.dispatch(action);
                self.run_commands();
                return Err(self.finish());
            }
            InputVerdict::Cancel => {
                self.cancel_input(&delivered);
                self.run_commands();
                return Err(self.finish());
            }
            InputVerdict::Replace(other) => other,
            _ => event,
        };
        Ok(event)
    }

    /// What the native input filters made of `event`: the input to go on with
    /// (as it was, or replaced), or the effects of one that was taken,
    /// cancelled or turned into an action.
    fn native_filtered(&mut self, now: Instant, event: InputEvent) -> Result<InputEvent, Effects> {
        use native_host::input_chain::Native;
        match self.native_input(now, &event) {
            Native::Pass => Ok(event),
            Native::Replaced(other) => Ok(other),
            Native::Taken => {
                if let InputEvent::Pointer(p) = event {
                    self.recognizers_lose(&p);
                }
                Err(self.finish())
            }
            Native::Cancelled => {
                self.cancel_input(&event);
                self.run_commands();
                Err(self.finish())
            }
            Native::Gesture(gesture) => {
                // The filter's gesture is the input: the fingers are let go.
                if let InputEvent::Pointer(p) = event {
                    self.recognizers_lose(&p);
                }
                self.recognized(input::Recognized::Gesture(gesture));
                self.run_commands();
                Err(self.finish())
            }
            Native::Raise(name) => {
                if let InputEvent::Pointer(p) = event {
                    self.recognizers_lose(&p);
                }
                match InputKey::parse(&name) {
                    Some(key) => self.route_key(key),
                    None => self.warn_runtime(format!("a filter raised unknown input `{name}`")),
                }
                self.run_commands();
                Err(self.finish())
            }
            Native::Run(action) => match Id::parse(&action) {
                Ok(action) => {
                    self.dispatch(action);
                    self.run_commands();
                    Err(self.finish())
                }
                Err(_) => Ok(event),
            },
        }
    }
}
