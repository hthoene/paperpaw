"""The schema tables of the generated record codecs and their one interpreter (#307).

A record's fields are a `*_FIELDS` table of four bytes a field; one decoder, one sizer
and one writer walk it. They replace the decoder, sizer and writer that were generated
per record: the bytes on the wire are the same, the code no longer grows with the
number of records.

The table of a record is made from its struct: `wire_fields!` checks a row's kind against
the Rust type of the struct's field and takes its place from `core::mem::offset_of!`,
both by the field's one name, so a table cannot describe another struct than its own.
"""

SCALAR_BITS = {'u32': 32, 'i32': 32, 'u64': 64}


def defaults_of(declared):
    """The defaults of optional numbers, as `WIRE_DEFAULTS` keeps them: 0 first."""
    values = [0]
    for record in declared:
        for field in record['fields']:
            if field['optional'] and not field['maybe'] and field['kind'] in SCALAR_BITS:
                value = field['default'] & ((1 << SCALAR_BITS[field['kind']]) - 1)
                if value not in values:
                    values.append(value)
    return values


def kind_of(field):
    """The kind of a field: the name of its `WireField` constructor."""
    if field['maybe']:
        return 'maybe_' + field['kind']
    return {'str': 'text', 'bytes': 'bytes'}.get(field['kind'], field['kind'])


def kinds_of(declared):
    """The kinds the fields of `declared` have."""
    return {kind_of(field) for record in declared for field in record['fields']}


def fields_table(record, title, prefix, life, defaults, indent=''):
    """The `*_FIELDS` table of one record (`wire_fields!`): a row is a field, its kind,
    its presence ordinal plus one (0: required), and its cap or the index of its default."""
    fields = record['fields']
    target = title + ("<'static>" if life else '')
    out = ['wire_fields! {',
           f'    /// The fields of `{record["name"]}` as the codec walks them.',
           f'    {prefix}_FIELDS: [{target}; {len(fields)}] {{']
    for field in fields:
        optional = field['ordinal'] + 1 if field['optional'] else 0
        extra = field['cap']
        if field['optional'] and not field['maybe'] and field['kind'] in SCALAR_BITS:
            extra = defaults.index(field['default'] & ((1 << SCALAR_BITS[field['kind']]) - 1))
        out.append(f'        {field["name"]}: {kind_of(field)}, {optional}, {extra};')
    out += ['    }', '}', '']
    return '\n'.join(indent + line if line else line for line in out)


# A kind: its `WireField` constructor's name, the Rust type of the struct's field, its bits.
KINDS = (('u32', 'u32', '0'), ('i32', 'i32', 'Self::SIGNED'), ('u64', 'u64', 'Self::LONG'),
         ('text', "&'static str", 'Self::BLOB | Self::TEXT'),
         ('bytes', "&'static [u8]", 'Self::BLOB'),
         ('maybe_u32', 'Option<u32>', 'Self::MAYBE'),
         ('maybe_i32', 'Option<i32>', 'Self::MAYBE | Self::SIGNED'),
         ('maybe_u64', 'Option<u64>', 'Self::MAYBE | Self::LONG'))


def interpreter_source(defaults, no_space, utf8, kinds, typed=True):
    """The table types and the interpreter; `no_space` and `utf8` are the statuses of a
    field over its cap and of text that is not UTF-8, `kinds` (`kinds_of`) the kinds a
    table names (each gets its constructor). `typed` adds the calls the typed codec of a
    record makes with its own table."""
    table = ', '.join(str(value) for value in defaults)
    makers = ''.join(_maker(name, rust, bits) for name, rust, bits in KINDS if name in kinds)
    return ((INTERPRETER_TEMPLATE + (TYPED_CALLS if typed else ''))
            .replace('@MAKERS@', makers)
            .replace('@DEFAULTS@', table)
            .replace('@DEFAULT_COUNT@', str(len(defaults)))
            .replace('@NO_SPACE@', str(no_space)).replace('@UTF8@', str(utf8)))


def _maker(name, rust, bits):
    """The constructor of one kind, laid out as rustfmt does (100 columns)."""
    head = f'    const fn {name}<R>('
    params = [f'_: fn(&R) -> &{rust}', 'offset: usize', 'optional: u8', 'cap: usize']
    line = head + ', '.join(params) + ') -> Self {'
    if len(line) > 100:
        line = head + '\n' + ''.join(f'        {param},\n' for param in params) + '    ) -> Self {'
    return (f'\n    /// A field of the Rust type `{rust}`.\n{line}\n'
            f'        Self::new({bits}, offset, optional, cap)\n    }}\n')


INTERPRETER_TEMPLATE = r"""
// The schema tables and their interpreter: one decoder, one sizer and one writer walk a
// record's `*_FIELDS` table.

/// One field of a record: what it holds, where it lies in the record's struct and how
/// far it may grow.
#[derive(Clone, Copy)]
struct WireField {
    /// Its kind in bits 0-3 (`LONG`, `BLOB`, `TEXT`/`SIGNED`, `MAYBE`) and, for an
    /// optional field, its presence ordinal plus one in bits 4-7 (0: a required field).
    shape: u8,
    /// Its offset in the struct, in units of four bytes.
    offset: u8,
    /// Text and bytes: the cap in bytes. An optional number with a default: the index of
    /// the default in `WIRE_DEFAULTS`.
    cap: u16,
}

/// The defaults of optional numbers (a field names one by its index).
static WIRE_DEFAULTS: [u64; @DEFAULT_COUNT@] = [@DEFAULTS@];

impl WireField {
    /// A number of eight bytes (`u64`), not of four.
    const LONG: u8 = 1;
    /// Text or bytes behind their length, not a number.
    const BLOB: u8 = 2;
    /// Of a blob: UTF-8 text (`&str`), not bytes (`&[u8]`).
    const TEXT: u8 = 4;
    /// Of a number of four bytes: an `i32`, not a `u32`.
    const SIGNED: u8 = 4;
    /// An optional number without a default: an `Option`.
    const MAYBE: u8 = 8;
    /// What a required `u32` or `i32` has no bit of: the fields most records are made of.
    const NOT_A_WORD: u8 = !Self::SIGNED;

    /// A row: `kind` bits, the field's offset in its struct, 0 for a required field or
    /// else its presence ordinal plus one, and its cap or the index of its default.
    const fn new(kind: u8, offset: usize, optional: u8, cap: usize) -> Self {
        assert!(offset % 4 == 0 && offset / 4 <= 0xff && optional <= 8 && cap <= 0xffff);
        Self {
            shape: kind | optional << 4,
            offset: (offset / 4) as u8,
            cap: cap as u16,
        }
    }

    // One constructor for each kind a table names. Its first argument reads the field
    // from the struct `R` (`wire_fields!` gives it and the offset from one name), so a
    // row's kind is checked against the type of the struct's own field.
@MAKERS@
    /// Whether the record may leave it out.
    fn optional(self) -> bool {
        self.shape >> 4 != 0
    }

    /// Its bit of the presence bitmap; 0 for a required field.
    fn mask(self) -> u8 {
        ((1_u16 << (self.shape >> 4)) >> 1) as u8
    }

    /// The value an optional number has when it is left out.
    fn default(self) -> u64 {
        WIRE_DEFAULTS
            .get(usize::from(self.cap))
            .copied()
            .unwrap_or(0)
    }
}

/// A record's `*_FIELDS` table. A row is a field of the struct, its kind (the name of
/// its `WireField` constructor), its presence ordinal plus one (0: a required field),
/// and its cap in bytes (text and bytes) or the index of its default in `WIRE_DEFAULTS`
/// (an optional number).
macro_rules! wire_fields {
    ($(#[$doc:meta])* $name:ident: [$record:ty; $count:expr] {
        $($field:ident: $kind:ident, $optional:expr, $cap:expr;)*
    }) => {
        $(#[$doc])*
        static $name: [WireField; $count] = [$(WireField::$kind(
            |record: &$record| &record.$field,
            core::mem::offset_of!($record, $field),
            $optional,
            $cap,
        )),*];
    };
}

/// A field's value as the wire frames it.
enum WireValue<'r> {
    Word(u32),
    Long(u64),
    Blob(&'r [u8]),
}

/// Decodes a record body (its presence bitmap, then its fields) into the struct at
/// `out`. Unknown appended fields and bitmap bytes are skipped.
///
/// The kinds are told apart by their bits, one test after the other, not by a `match`:
/// no jump table, so the firmware's frame scan reads these functions to their end.
///
/// # Safety
///
/// `fields` is the `*_FIELDS` table of the struct `out` points to, and `out` is valid
/// for writing that struct. With `Ok`, every field of the struct is written, and its
/// borrows live as long as `body`.
#[inline(never)]
unsafe fn fields_decode<'a>(fields: &[WireField], body: &'a [u8], out: *mut u8) -> Result<(), i32> {
    let mut body = Reader { remaining: body };
    let bits = body.bitmap_bits()?;
    for &field in fields {
        let shape = field.shape;
        // SAFETY: the offset is that of a field of the struct, and the field's type is
        // the one its kind names (its `WireField` constructor), so each write is in
        // bounds, aligned and of the field's own type (an `i32` takes the four bytes of
        // a `u32`).
        unsafe {
            let at = out.add(usize::from(field.offset) * 4);
            if shape & WireField::NOT_A_WORD == 0 {
                at.cast::<u32>().write(u32::from_le_bytes(body.scalar()?));
                continue;
            }
            let mask = field.mask();
            let present = mask == 0 || bits & mask != 0;
            if shape & WireField::BLOB != 0 {
                let cap = usize::from(field.cap);
                if shape & WireField::TEXT != 0 {
                    at.cast::<&'a str>()
                        .write(if present { body.string(cap)? } else { "" });
                } else {
                    at.cast::<&'a [u8]>()
                        .write(if present { body.blob(cap)? } else { &[] });
                }
            } else if shape & WireField::MAYBE != 0 {
                if shape & WireField::LONG != 0 {
                    at.cast::<Option<u64>>().write(if present {
                        Some(u64::from_le_bytes(body.scalar()?))
                    } else {
                        None
                    });
                } else if shape & WireField::SIGNED != 0 {
                    at.cast::<Option<i32>>().write(if present {
                        Some(i32::from_le_bytes(body.scalar()?))
                    } else {
                        None
                    });
                } else {
                    at.cast::<Option<u32>>().write(if present {
                        Some(u32::from_le_bytes(body.scalar()?))
                    } else {
                        None
                    });
                }
            } else if shape & WireField::LONG != 0 {
                at.cast::<u64>().write(if present {
                    u64::from_le_bytes(body.scalar()?)
                } else {
                    field.default()
                });
            } else {
                at.cast::<u32>().write(if present {
                    u32::from_le_bytes(body.scalar()?)
                } else {
                    field.default() as u32
                });
            }
        }
    }
    Ok(())
}

/// [`fields_decode`] into a new `T`.
///
/// # Safety
///
/// `fields` is the `*_FIELDS` table of `T`, and `T` borrows from nothing but `body`.
#[inline(always)]
unsafe fn fields_decode_as<T>(fields: &[WireField], body: &[u8]) -> Result<T, i32> {
    let mut record = core::mem::MaybeUninit::<T>::uninit();
    // SAFETY: the table is `T`'s (the caller's word), `record` has room for a `T`, and
    // with `Ok` every field of it is written.
    unsafe {
        fields_decode(fields, body, record.as_mut_ptr().cast())?;
        Ok(record.assume_init())
    }
}

/// The value of `field` in the struct at `record`; `None` where an optional field is
/// absent or at its default, which the wire leaves out.
///
/// # Safety
///
/// `field` is a row of the `*_FIELDS` table of the struct `record` points to, which is
/// valid for reads for `'r`.
#[inline(always)]
unsafe fn field_value<'r>(field: WireField, record: *const u8) -> Option<WireValue<'r>> {
    let shape = field.shape;
    let optional = field.optional();
    // SAFETY: as in `fields_decode`: the offset and the type read are the field's own.
    unsafe {
        let at = record.add(usize::from(field.offset) * 4);
        if shape & WireField::BLOB != 0 {
            let value = if shape & WireField::TEXT != 0 {
                at.cast::<&'r str>().read().as_bytes()
            } else {
                at.cast::<&'r [u8]>().read()
            };
            (!optional || !value.is_empty()).then_some(WireValue::Blob(value))
        } else if shape & WireField::MAYBE != 0 {
            if shape & WireField::LONG != 0 {
                at.cast::<Option<u64>>().read().map(WireValue::Long)
            } else if shape & WireField::SIGNED != 0 {
                at.cast::<Option<i32>>()
                    .read()
                    .map(|value| WireValue::Word(value as u32))
            } else {
                at.cast::<Option<u32>>().read().map(WireValue::Word)
            }
        } else if shape & WireField::LONG != 0 {
            let value = at.cast::<u64>().read();
            (!optional || value != field.default()).then_some(WireValue::Long(value))
        } else {
            let value = at.cast::<u32>().read();
            (!optional || value != field.default() as u32).then_some(WireValue::Word(value))
        }
    }
}

/// The body length of the record at `record`: its presence bitmap and its fields. Text
/// or bytes over their cap are refused.
///
/// # Safety
///
/// `fields` is the `*_FIELDS` table of the struct `record` points to, which is valid
/// for reads.
#[inline(never)]
unsafe fn fields_len(fields: &[WireField], record: *const u8) -> Result<usize, i32> {
    // The bitmap's length byte; the bitmap itself is one byte when any optional field
    // is present.
    let mut len = 1;
    let mut bitmap = 0;
    for &field in fields {
        // A required number: its size, without a look at the record.
        if field.shape & !(WireField::SIGNED | WireField::LONG) == 0 {
            len += 4 << (field.shape & WireField::LONG);
            continue;
        }
        // SAFETY: the caller's word, for each row of the table.
        let Some(value) = (unsafe { field_value(field, record) }) else {
            continue;
        };
        bitmap |= usize::from(field.optional());
        len += match value {
            WireValue::Word(_) => 4,
            WireValue::Long(_) => 8,
            WireValue::Blob(bytes) => {
                if bytes.len() > usize::from(field.cap) {
                    return Err(@NO_SPACE@);
                }
                varint_len(bytes.len() as u32) + bytes.len()
            }
        };
    }
    Ok(len + bitmap)
}

/// Writes the body of the record at `record`: its presence bitmap, then its fields.
/// The caller has checked that [`fields_len`] bytes fit.
///
/// # Safety
///
/// `fields` is the `*_FIELDS` table of the struct `record` points to, which is valid
/// for reads.
#[inline(never)]
unsafe fn fields_write(
    fields: &[WireField],
    record: *const u8,
    out: &mut [u8],
    offset: &mut usize,
) {
    let mut bits = 0;
    // Optional fields are a record's last ones.
    for &field in fields.iter().rev() {
        if !field.optional() {
            break;
        }
        // SAFETY: the caller's word, for each row of the table.
        if unsafe { field_value(field, record) }.is_some() {
            bits |= field.mask();
        }
    }
    write_varint(u32::from(bits != 0), out, offset);
    if bits != 0 {
        write_bytes(&[bits], out, offset);
    }
    for &field in fields {
        if field.shape & WireField::NOT_A_WORD == 0 {
            // SAFETY: the caller's word: a required `u32` or `i32` of the struct, both
            // four bytes that every bit pattern is a value of.
            let value = unsafe {
                record
                    .add(usize::from(field.offset) * 4)
                    .cast::<u32>()
                    .read()
            };
            write_bytes(&value.to_le_bytes(), out, offset);
            continue;
        }
        // SAFETY: the caller's word, for each row of the table.
        match unsafe { field_value(field, record) } {
            Some(WireValue::Word(value)) => write_bytes(&value.to_le_bytes(), out, offset),
            Some(WireValue::Long(value)) => write_bytes(&value.to_le_bytes(), out, offset),
            Some(WireValue::Blob(bytes)) => {
                write_varint(bytes.len() as u32, out, offset);
                write_bytes(bytes, out, offset);
            }
            None => {}
        }
    }
}
"""

TYPED_CALLS = r"""
/// [`fields_len`] of `record`.
///
/// # Safety
///
/// `fields` is the `*_FIELDS` table of `T`.
#[inline(always)]
unsafe fn fields_len_of<T>(fields: &[WireField], record: &T) -> Result<usize, i32> {
    // SAFETY: the table is `T`'s (the caller's word), and `record` is a `T`.
    unsafe { fields_len(fields, core::ptr::from_ref(record).cast()) }
}

/// [`fields_write`] of `record`.
///
/// # Safety
///
/// `fields` is the `*_FIELDS` table of `T`.
#[inline(always)]
unsafe fn fields_write_of<T>(fields: &[WireField], record: &T, out: &mut [u8], offset: &mut usize) {
    // SAFETY: the table is `T`'s (the caller's word), and `record` is a `T`.
    unsafe { fields_write(fields, core::ptr::from_ref(record).cast(), out, offset) }
}
"""

# Text of a record body: ids, paths and keys are ASCII, which is checked a word at a
# time; other text goes the whole way.
TEXT_SOURCE = '''/// `bytes` as text; `status` where they are not UTF-8.
fn wire_text(bytes: &[u8], status: i32) -> Result<&str, i32> {
    if bytes.is_ascii() {
        // SAFETY: ASCII is UTF-8.
        Ok(unsafe { core::str::from_utf8_unchecked(bytes) })
    } else {
        core::str::from_utf8(bytes).map_err(|_| status)
    }
}
'''
