//! Handover requests use the generated service-op row cap, not registration caps.
use super::{Error, HandoverId, Host, Param, Ui, UiPage, expect, one, params, schema, wire};

impl Ui {
    /// Make a handover with at most 32 unique params and 1,024 content bytes.
    /// The params are encoded straight into `input`: nothing is allocated
    /// and no record array is placed on the X4 callback stack.
    ///
    /// # Errors
    /// Invalid for duplicate keys; NoSpace above caps; otherwise the host's
    /// refusal.
    pub fn handover(
        &self,
        host: &Host<'_>,
        values: &[Param<'_>],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<HandoverId, Error> {
        let op = self.operation(schema::OP_HANDOVER)?;
        params::validate(values, usize::from(op.input_rows))?;
        let count = wire::records_encode_counted_of(
            values.iter().map(|value| value.record()),
            usize::from(op.input_rows),
            op.max_input,
            input,
        )
        .map_err(Error::from_status)?;
        let count = self
            .service
            .call(host, schema::OP_HANDOVER, &input[..count], out)?;
        let row: wire::ServiceUiHandle = expect(one(&out[..count])?)?;
        HandoverId::new(row.handle).ok_or(Error::Invalid)
    }

    /// Read all params of a handover assigned to this instance. A handover
    /// holds at most 32 rows; the host returns its bare stream followed by end.
    ///
    /// # Errors
    /// Returns State for a mismatched reply or the host's ownership refusal.
    pub fn params<'a>(
        &self,
        host: &Host<'_>,
        handover: HandoverId,
        input: &mut [u8],
        out: &'a mut [u8],
    ) -> Result<UiPage<'a, Param<'a>>, Error> {
        let count = self.call(
            host,
            schema::OP_PARAMS,
            &[&wire::ServiceUiHandle {
                handle: handover.get(),
            }],
            input,
            out,
        )?;
        UiPage::terminal(
            &out[..count],
            self.operation(schema::OP_PARAMS)?,
            Param::decode,
        )
    }
}
