//! Stage declarations and bounded subscriber registration.
use super::{Declarations, DeclareError, Item, Latched, filled, records_wire, within};
use paperpaw_native::host_glue::ERR_INVALID;

const MAX_REPLY_RECORDS: u32 = 64;

/// A stage owned by this plugin, with bounded opaque input and replies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageDecl<'a> {
    name: &'a str,
    schema: u32,
    flags: u32,
    max_input: u32,
    max_reply: u32,
    max_records: u32,
}

impl<'a> StageDecl<'a> {
    /// Declare a relative stage name and the limits of its payload schema.
    #[must_use]
    pub fn new(
        name: &'a str,
        schema: u32,
        max_input: u32,
        max_reply: u32,
        max_records: u32,
    ) -> Self {
        Self {
            name,
            schema,
            flags: 0,
            max_input,
            max_reply,
            max_records,
        }
    }

    /// Include subscriber signatures in the owner's layout key.
    #[must_use]
    pub fn changes_layout(mut self) -> Self {
        self.flags |= 1;
        self
    }

    /// Declare that subscribers may draw.
    #[must_use]
    pub fn draws(mut self) -> Self {
        self.flags |= 1 << 1;
        self
    }

    /// Declare that subscribers may handle taps.
    #[must_use]
    pub fn takes_taps(mut self) -> Self {
        self.flags |= 1 << 2;
        self
    }

    pub(super) fn record(self) -> records_wire::StageSpec<'a> {
        records_wire::StageSpec {
            name: self.name,
            schema: self.schema,
            flags: self.flags,
            max_input: self.max_input,
            max_reply: self.max_reply,
            max_records: self.max_records,
        }
    }
}

/// Subscribe a plugin-owned callback token to another plugin's stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubscribeDecl<'a> {
    stage: &'a str,
    token: u32,
    order: i16,
    signature: u64,
}

impl<'a> SubscribeDecl<'a> {
    /// Subscribe to a stage's full id, initially at order and signature zero.
    #[must_use]
    pub fn new(stage: &'a str, token: u32) -> Self {
        Self {
            stage,
            token,
            order: 0,
            signature: 0,
        }
    }

    /// Sort before subscribers with a larger order.
    #[must_use]
    pub fn order(mut self, order: i16) -> Self {
        self.order = order;
        self
    }

    /// The value owners fold into their layout key; change when output changes.
    #[must_use]
    pub fn signature(mut self, signature: u64) -> Self {
        self.signature = signature;
        self
    }

    pub(super) fn record(self) -> records_wire::RegistrationStage<'a> {
        records_wire::RegistrationStage {
            stage: self.stage,
            token: self.token,
            order: i32::from(self.order),
            signature: self.signature,
        }
    }
}

fn valid_id(id: &str) -> bool {
    id.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    })
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Declare an extension point owned by this plugin.
    ///
    /// # Errors
    /// Refuses a malformed name or limits outside the stage contract.
    /// A refusal poisons this construction, like every other declaration.
    pub fn stage(&mut self, stage: StageDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            filled(stage.name, "stage name")?;
            within(
                stage.name,
                records_wire::STAGE_SPEC_NAME_MAX_BYTES,
                "stage name",
            )?;
            if !valid_id(stage.name)
                || !(1..=records_wire::STAGE_CALL_PAYLOAD_MAX_BYTES as u32)
                    .contains(&stage.max_input)
                || !(1..=records_wire::STAGE_REPLY_PAYLOAD_MAX_BYTES as u32)
                    .contains(&stage.max_reply)
                || !(1..=MAX_REPLY_RECORDS).contains(&stage.max_records)
            {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            self.push(Item::Stage(stage))
        })();
        self.finish_stage_add(result)
    }

    /// Subscribe a callback token to a stage by full id.
    ///
    /// # Errors
    /// Refuses malformed ids. The host checks the eight-per-plugin limit,
    /// duplicate stage/token pairs and self-subscription when committing the batch.
    pub fn subscribe(&mut self, to: SubscribeDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.check_sink()?;
        let result = (|| {
            filled(to.stage, "subscription stage")?;
            within(
                to.stage,
                records_wire::REGISTRATION_STAGE_STAGE_MAX_BYTES,
                "subscription stage",
            )?;
            if !valid_id(to.stage) {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            self.push(Item::Subscribe(to))?;
            Ok(())
        })();
        self.finish_stage_add(result)
    }

    fn finish_stage_add(
        &mut self,
        result: Result<(), DeclareError>,
    ) -> Result<&mut Self, DeclareError> {
        if let Err(error) = result {
            if self.failed.is_none() {
                self.failed = Some(Latched::Refused(error));
            }
            return Err(error);
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
