//! Handover requests use the generated service-op row cap, not registration caps.
use super::{Error, HandoverId, Host, Param, Ui, UiPage, one, params, schema, wire};
use alloc::vec::Vec;

impl Ui {
    /// Make a handover with at most 32 unique params and 1,024 content bytes.
    /// The temporary record list is allocated fallibly and freed before the
    /// host call; no large record array is placed on the X4 callback stack.
    ///
    /// # Errors
    /// Invalid for duplicate keys; NoSpace above caps; NoMemory for the
    /// temporary list; otherwise the host's refusal.
    pub fn handover(
        &self,
        host: &Host<'_>,
        values: &[Param<'_>],
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<HandoverId, Error> {
        params::validate(values)?;
        let op = super::super::wire::services_wire::service_schema(schema::KEY, schema::MAJOR)
            .and_then(|schema| schema.op(schema::OP_HANDOVER))
            .ok_or(Error::Unsupported)?;
        let count = {
            let mut records = Vec::new();
            records
                .try_reserve_exact(values.len())
                .map_err(|_| Error::NoMemory)?;
            records.extend(values.iter().map(|value| value.record()));
            super::super::wire::service_input_encode(op, &records, input)
                .map_err(Error::from_status)?
        };
        let count = self
            .service
            .call(host, schema::OP_HANDOVER, &input[..count], out)?;
        let wire::Record::ServiceUiHandle(row) = one(&out[..count])? else {
            return Err(Error::State);
        };
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
            &[wire::Record::ServiceUiHandle(wire::ServiceUiHandle {
                handle: handover.get(),
            })],
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
