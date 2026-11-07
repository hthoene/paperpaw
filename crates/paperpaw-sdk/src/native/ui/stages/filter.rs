use super::super::{Error, navigation::full_id};
use crate::native::{
    callbacks::{Input, InputGestureKind, KeyState, PointerPhase},
    declare::records_wire as wire,
    stage_schemas::core_input_filter_v1 as schema,
};

/// One checked input-filter call. Input strings borrow its stage payload.
#[derive(Clone, Copy, Debug)]
pub struct FilterCall<'a> {
    input: Input<'a>,
}
impl<'a> FilterCall<'a> {
    /// Decode exactly one generated core.input-filter input.
    ///
    /// # Errors
    /// Returns malformed framing/input errors or State for a mismatched record.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        let mut rows = schema::input_decode(bytes).map_err(Error::from_status)?;
        let row = rows
            .next()
            .transpose()
            .map_err(Error::from_status)?
            .ok_or(Error::Invalid)?;
        if rows.next().is_some() {
            return Err(Error::Invalid);
        }
        let schema::Record::FilterInput(row) = row else {
            return Err(Error::State);
        };
        let input = Input::decode(row.input)?;
        if !matches!(
            input,
            Input::Key { .. } | Input::Pointer { .. } | Input::Gesture { .. }
        ) {
            return Err(Error::Invalid);
        }
        Ok(Self { input })
    }
    /// The key, pointer or gesture the host is filtering.
    #[must_use]
    pub const fn input(self) -> Input<'a> {
        self.input
    }
}

/// One input-filter decision. The host enforces ownership and capture rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterDecision<'a> {
    /// Continue to later filters.
    Pass,
    /// Consume the input and its matching release.
    Take,
    /// Run a full action id.
    Run(&'a str),
    /// Cancel this input and its matching release.
    Cancel,
    /// Replace it for the later filters.
    Replace(Input<'a>),
    /// Raise one of this plugin's declared inputs or gestures.
    Raise(&'a str),
}
impl FilterDecision<'_> {
    /// Encode one generated verdict. Replacement encoding uses the caller's
    /// temporary input buffer and retains no reference to it after return.
    ///
    /// # Errors
    /// Returns local caps/syntax errors; no text or input is truncated.
    pub fn encode(self, input_scratch: &mut [u8], out: &mut [u8]) -> Result<usize, Error> {
        let (kind, action, input, raise) = match self {
            Self::Pass => (0, "", &[][..], ""),
            Self::Take => (1, "", &[][..], ""),
            Self::Cancel => (3, "", &[][..], ""),
            Self::Run(action) => {
                full_id(action)?;
                (2, action, &[][..], "")
            }
            Self::Raise(input) => {
                full_id(input)?;
                (5, "", &[][..], input)
            }
            Self::Replace(input) => {
                let record = input_record(input)?;
                let limit = input_scratch
                    .len()
                    .min(schema::FILTER_VERDICT_INPUT_MAX_BYTES);
                let count = wire::records_encode(&[record], &mut input_scratch[..limit])
                    .map_err(Error::from_status)?;
                (4, "", &input_scratch[..count], "")
            }
        };
        schema::reply_encode(
            &[schema::Record::FilterVerdict(schema::FilterVerdict::new(
                kind, action, input, raise,
            ))],
            out,
        )
        .map_err(Error::from_status)
    }
}
fn input_record(input: Input<'_>) -> Result<wire::Record<'_>, Error> {
    Ok(match input {
        Input::Key { now_ms, key, state } => wire::Record::InputKey(wire::InputKey {
            now_ms,
            key,
            state: match state {
                KeyState::Press => 0,
                KeyState::Release => 1,
                KeyState::Repeat => 2,
            },
        }),
        Input::Pointer {
            now_ms,
            position,
            phase,
        } => wire::Record::InputPointer(wire::InputPointer {
            now_ms,
            x: position.x,
            y: position.y,
            phase: match phase {
                PointerPhase::Down => 0,
                PointerPhase::Move => 1,
                PointerPhase::Up => 2,
                PointerPhase::Cancel => 3,
            },
        }),
        Input::Gesture {
            now_ms,
            position,
            kind,
        } => wire::Record::InputGesture(wire::InputGesture {
            now_ms,
            x: position.x,
            y: position.y,
            kind: match kind {
                InputGestureKind::Tap => 0,
                InputGestureKind::LongPress => 1,
                InputGestureKind::SwipeLeft => 2,
                InputGestureKind::SwipeRight => 3,
                InputGestureKind::SwipeUp => 4,
                InputGestureKind::SwipeDown => 5,
            },
        }),
        _ => return Err(Error::Invalid),
    })
}
