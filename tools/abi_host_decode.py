"""The host's decoder section of records_wire.rs (lock format 10, #985).

A host decodes only records a plugin sends it: registrations, declarations,
format arguments, stage replies and service inputs. Records only the host sends
(events, input, frames, service replies, stage calls) are refused by tag before
their body is read, so a host that decodes through this section links no
decoder for them. The section is separate from the records section, so that
section re-renders byte-identical for earlier locks.
"""
from abi_declarations import _camel, _const, wire_records

# Families whose records only the host sends.
HOST_SENT_FAMILIES = ('event', 'input', 'draw')
# Records only the host sends, in families a plugin also sends.
# The host writes both stage records (`stage.call` to a subscriber, `stage.reply`
# into the caller's `out`); subscribers answer in their stage's own tags.
HOST_SENT_RECORDS = ('stage.call', 'stage.reply')


def service_inputs(schema):
    """Every service record a schema op takes as input: a plugin sends it."""
    return {name for service in schema.get('services') or []
            for op in service.get('ops', []) for name in op.get('input', [])}


def host_sent(schema, name):
    """Whether only the host sends this record, so the host never decodes it."""
    family = name.split('.')[0]
    if family == 'service':
        return name not in service_inputs(schema)
    return family in HOST_SENT_FAMILIES or name in HOST_SENT_RECORDS


def rust_host_decode(schema):
    """The host decoder section; empty without declaration records."""
    declared = wire_records(schema)
    if not declared:
        return ''
    statuses = {item['name']: item['value'] for item in schema['statuses']}
    sent = [r for r in declared if host_sent(schema, r['name'])]
    taken = [r for r in declared if not host_sent(schema, r['name'])]
    arms = ''.join(f"        {_const(r['name'])}_TAG => {_camel(r['name'])}::decode(body)?,\n"
                   for r in taken)
    # `event.tick` has its own codec (not a wire record) and only the host sends it.
    refused = ['EVENT_TICK_TAG'] + [_const(r['name']) + '_TAG' for r in sent]
    arms += '        ' + '\n        | '.join(refused) + ' => return Err(@INVALID@),\n'
    arguments = [r for r in declared if r['name'].startswith('i18n.')]
    argument_arms = ''.join(f"        {_const(r['name'])}_TAG => {_camel(r['name'])}::decode(body)?,\n"
                            for r in arguments)
    source = HOST_TEMPLATE.replace('@ARMS@', arms.rstrip('\n')).replace(
        '@ARGUMENTS@', ARGUMENT_TEMPLATE.replace('@ARMS@', argument_arms.rstrip('\n')))
    for marker, name in [('@INVALID@', 'ERR_INVALID'), ('@UNSUPPORTED@', 'ERR_UNSUPPORTED'),
                         ('@NO_SPACE@', 'ERR_NO_SPACE')]:
        source = source.replace(marker, str(statuses[name]))
    return source


ARGUMENT_TEMPLATE = '''/// Decode a format argument (`i18n.*`); any other tag is ERR_UNSUPPORTED
/// before its body is read, as slot 27 answers for a record that is not one.
fn host_argument<'a>(tag: u32, body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
    Ok(match tag {
@ARMS@
        _ => return Err(@UNSUPPORTED@),
    })
}
'''

HOST_TEMPLATE = '''
// The host's decoders (lock format 10): records a plugin sends, nothing else.

/// Decode a record a plugin sends the host. A record only the host sends (an
/// event, input, a frame, a service reply, a stage call) is ERR_INVALID before
/// its body is read; an unknown tag is ERR_UNSUPPORTED. Other statuses are
/// those of [`records_decode`].
fn host_decode<'a>(tag: u32, body: &mut Reader<'a>) -> Result<Record<'a>, i32> {
    Ok(match tag {
        REGISTRATION_EVENT_TAG => {
            body.bitmap()?;
            Record::Registration(Registration::Event {
                event_tag: u32::from_le_bytes(body.scalar()?),
                token: u32::from_le_bytes(body.scalar()?),
            })
        }
        REGISTRATION_DRAW_TAG => {
            body.bitmap()?;
            Record::Registration(Registration::Draw {
                token: u32::from_le_bytes(body.scalar()?),
            })
        }
@ARMS@
        _ => return Err(@UNSUPPORTED@),
    })
}

@ARGUMENTS@
impl RecordBatch<'_> {
    /// A batch without records, to decode into ([`host_records_decode_into`]).
    pub const EMPTY: Self = Self {
        records: [None; REGISTRATION_MAX_RECORDS],
        len: 0,
    };
}

/// [`records_decode`] as the host decodes a plugin's batch, into a batch the
/// caller holds (for example on the heap, so it never passes through the
/// stack). Records only the host sends are ERR_INVALID ([`host_decode_one`]).
/// On an error the batch is empty.
pub fn host_records_decode_into<'a>(
    bytes: &'a [u8],
    batch: &mut RecordBatch<'a>,
) -> Result<(), i32> {
    host_records_decode_with(bytes, batch, host_decode)
}

/// A format argument batch (slot 27) as the host decodes it: `i18n.*` records
/// only; any other tag is ERR_UNSUPPORTED before its body is read. On an error
/// the batch is empty.
pub fn host_arguments_decode_into<'a>(
    bytes: &'a [u8],
    batch: &mut RecordBatch<'a>,
) -> Result<(), i32> {
    host_records_decode_with(bytes, batch, host_argument)
}

fn host_records_decode_with<'a>(
    bytes: &'a [u8],
    batch: &mut RecordBatch<'a>,
    decode: HostDecoder,
) -> Result<(), i32> {
    let decoded = host_records_fill(bytes, batch, decode);
    if decoded.is_err() {
        batch.len = 0;
        for place in batch.records.iter_mut() {
            *place = None;
        }
    }
    decoded
}

/// One record of a plugin's batch, decoded by tag ([`host_decode`] or [`host_argument`]).
type HostDecoder = for<'a> fn(u32, &mut Reader<'a>) -> Result<Record<'a>, i32>;

fn host_records_fill<'a>(
    bytes: &'a [u8],
    batch: &mut RecordBatch<'a>,
    decode: HostDecoder,
) -> Result<(), i32> {
    batch.len = 0;
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    let len = reader.varint()? as usize;
    if len > REGISTRATION_MAX_RECORDS {
        return Err(@NO_SPACE@);
    }
    for (index, place) in batch.records.iter_mut().enumerate() {
        *place = if index < len {
            let (tag, mut body) = reader.record()?;
            Some(decode(tag, &mut body)?)
        } else {
            None
        };
    }
    reader.finish()?;
    batch.len = len;
    Ok(())
}

/// [`record_decode_prefix`] as the host decodes a record a plugin sent (a
/// stored declaration): records only the host sends are ERR_INVALID.
pub fn host_decode_prefix(bytes: &[u8]) -> Result<(Record<'_>, usize), i32> {
    let limit = bytes.len().min(REGISTRATION_MAX_BYTES);
    let mut reader = Reader {
        remaining: bytes.get(..limit).unwrap_or(bytes),
    };
    let (tag, mut body) = reader.record()?;
    let record = host_decode(tag, &mut body)?;
    Ok((record, limit - reader.remaining.len()))
}

/// [`record_decode_one`] as the host decodes a record a plugin sent (a stage
/// reply, a service input): records only the host sends are ERR_INVALID.
pub fn host_decode_one(bytes: &[u8]) -> Result<Record<'_>, i32> {
    if bytes.len() > REGISTRATION_MAX_BYTES {
        return Err(@NO_SPACE@);
    }
    let mut reader = Reader { remaining: bytes };
    let (tag, mut body) = reader.record()?;
    let record = host_decode(tag, &mut body)?;
    reader.finish()?;
    Ok(record)
}
'''
