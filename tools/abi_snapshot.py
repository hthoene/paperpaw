"""Validate and emit the bounded environment snapshot wire contract."""

HEADER = [('version', 'u32'), ('total_bytes', 'u32'), ('presence', 'u32'),
          ('reserved', 'u32')]
FIELDS = [('monotonic_ms', 'u64'), ('unix_seconds', 'i64'), ('width', 'u16'),
          ('height', 'u16'), ('dpi', 'u16'), ('pixel_kind', 'u16'),
          ('pixel_code', 'u16'), ('pixel_bits', 'u8'), ('percent', 'u8'),
          ('millivolts', 'u16'), ('charging', 'u8'), ('saver', 'u8'),
          ('touch', 'u8'), ('reserved_tail', 'bytes3')]
PIXELS = [('mono1', 'Mono1', 1), ('gray2', 'Gray2', 2), ('gray4', 'Gray4', 4),
          ('gray8', 'Gray8', 8), ('rgb565', 'Rgb565', 16), ('rgb888', 'Rgb888', 24),
          ('other', 'Other', 0)]
SIZES = {'u8': 1, 'u16': 2, 'u32': 4, 'u64': 8, 'i64': 8, 'bytes3': 3}


def _require(ok, message):
    if not ok:
        raise ValueError(message)


def _integer(value, low, high):
    _require(type(value) is int and low <= value <= high,
             'snapshot integer outside bounds')


def validate_snapshot(schema):
    """Older schemas omit the descriptor; the first layout has a closed shape."""
    if 'snapshot_value' not in schema:
        return
    descriptor = schema['snapshot_value']
    _require(type(descriptor) is dict and set(descriptor) ==
             set('since version byte_order max_bytes header fields presence pixels'.split()),
             'snapshot descriptor keys')
    _integer(descriptor['since'], 1, schema['minor'])
    _integer(descriptor['version'], 1, 1)
    _require(descriptor['byte_order'] == 'little', 'snapshot byte order')
    _integer(descriptor['max_bytes'], 128, 128)
    for key, expected in [('header', HEADER), ('fields', FIELDS)]:
        _require(descriptor[key] == [dict(name=n, type=t) for n, t in expected],
                 'snapshot ' + key + ' shape')
    presence = descriptor['presence']
    _require(type(presence) is list and len(presence) == 3, 'snapshot presence shape')
    for item, (name, bit) in zip(presence, [('battery', 1), ('voltage', 2), ('unix', 4)]):
        _require(type(item) is dict and set(item) == {'name', 'bit'} and
                 item['name'] == name, 'snapshot presence shape')
        _integer(item['bit'], bit, bit)
    pixels = descriptor['pixels']
    _require(type(pixels) is list and len(pixels) == len(PIXELS), 'snapshot pixel shape')
    for item, (name, _, bits) in zip(pixels, PIXELS):
        _require(type(item) is dict and set(item) == {'name', 'tag', 'bits'} and
                 item['name'] == name, 'snapshot pixel shape')
        _integer(item['tag'], 0, 65535)
        _integer(item['bits'], bits, bits)
    _require(len({item['tag'] for item in pixels}) == len(pixels), 'snapshot duplicate pixel tag')
    for name, value in [('ERR_UNSUPPORTED', -1), ('ERR_INVALID', -2), ('ERR_NO_SPACE', -3)]:
        entries = [item for item in schema.get('statuses', []) if item.get('name') == name]
        _require(len(entries) == 1 and type(entries[0].get('value')) is int and
                 entries[0]['value'] == value, 'snapshot status contract')
        _integer(entries[0].get('since'), 1, descriptor['since'])
    entries = [item for item in schema.get('host_entries', [])
               if item.get('name') == 'env.snapshot']
    _require(len(entries) == 1 and type(entries[0].get('max_bytes')) is int and
             entries[0]['max_bytes'] == descriptor['max_bytes'] and
             entries[0].get('id') == 4 and entries[0].get('result') == 'status',
             'snapshot slot/cap contract')
    _integer(entries[0].get('since'), 1, descriptor['since'])


def compatible_snapshot(old, new, minimum_since):
    """An existing descriptor is immutable; its first addition respects the floor."""
    validate_snapshot(old)
    validate_snapshot(new)
    _integer(minimum_since, 1, 65536)
    before, after = old.get('snapshot_value'), new.get('snapshot_value')
    if before is None:
        if after is not None:
            _require(after['since'] >= minimum_since, 'backdated snapshot value')
        return
    _require(after == before, 'changed or removed snapshot value contract')


def rust_snapshot(schema):
    """Emit an independent allocation-free no_std module, with derived offsets."""
    validate_snapshot(schema)
    if 'snapshot_value' not in schema:
        return ''
    descriptor = schema['snapshot_value']
    replacements = {'@VERSION@': descriptor['version'], '@MAX@': descriptor['max_bytes']}
    offset = 0
    for field in descriptor['header'] + descriptor['fields']:
        name, kind = field['name'].upper(), field['type']
        replacements['@' + name + '_OFFSET@'] = offset
        offset += SIZES[kind]
        replacements['@' + name + '_END@'] = offset
    replacements['@BYTES@'] = offset
    for item in descriptor['presence']:
        replacements['@P_' + item['name'].upper() + '@'] = item['bit']
    for item in descriptor['pixels']:
        replacements['@K_' + item['name'].upper() + '@'] = item['tag']
    for item in schema['statuses']:
        replacements['@' + item['name'] + '@'] = item['value']
    source = RUST
    for marker, value in replacements.items():
        source = source.replace(marker, str(value))
    return source


RUST = '''// Generated by tools/abi-gen.py; do not edit.
/// Environment snapshot wire layout version.
pub const SNAPSHOT_VERSION: u32 = @VERSION@;
/// Complete canonical snapshot prefix length.
pub const SNAPSHOT_BYTES: usize = @BYTES@;
/// Maximum snapshot length, including same-version extensions.
pub const SNAPSHOT_MAX_BYTES: usize = @MAX@;
const BATTERY: u32 = @P_BATTERY@;
const VOLTAGE: u32 = @P_VOLTAGE@;
const UNIX: u32 = @P_UNIX@;

/// A local, owned view of the environment. No Rust layout crosses the ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Snapshot {
    /// Monotonic milliseconds since boot.
    pub monotonic_ms: u64,
    /// Signed Unix seconds, when available.
    pub unix_seconds: Option<i64>,
    /// Battery data, when available.
    pub battery: Option<Battery>,
    /// Display width in pixels.
    pub width: u16,
    /// Display height in pixels.
    pub height: u16,
    /// Display pixels per inch.
    pub dpi: u16,
    /// Whether touch input is available.
    pub touch: bool,
    /// Display pixel format.
    pub pixel: PixelFormat,
}

impl Snapshot {
    /// Construct a snapshot with no clock or battery data and no touch input.
    pub const fn new(width: u16, height: u16, dpi: u16, pixel: PixelFormat) -> Self {
        Self {
            monotonic_ms: 0,
            unix_seconds: None,
            battery: None,
            width,
            height,
            dpi,
            touch: false,
            pixel,
        }
    }
}

/// Optional battery data. Zero voltage is distinct from unavailable voltage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Battery {
    /// Charge percentage, in 0..=100.
    pub percent: u8,
    /// Whether external power is charging the battery.
    pub charging: bool,
    /// Whether battery saving mode is active.
    pub saver: bool,
    /// Battery voltage in millivolts, when available.
    pub millivolts: Option<u16>,
}

impl Battery {
    /// Construct battery data without voltage or active power flags.
    pub const fn new(percent: u8) -> Self {
        Self {
            percent,
            charging: false,
            saver: false,
            millivolts: None,
        }
    }
}

/// Pixel formats with the same derived depth semantics as the SDK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PixelFormat {
    /// One bit monochrome.
    Mono1,
    /// Two bit grayscale.
    Gray2,
    /// Four bit grayscale.
    Gray4,
    /// Eight bit grayscale.
    Gray8,
    /// Sixteen bit RGB colour.
    Rgb565,
    /// Twenty-four bit RGB colour.
    Rgb888,
    /// A shell-defined format, including an unspecified zero code or depth.
    Other {
        /// Shell-defined format code.
        code: u16,
        /// Bits per pixel.
        bits: u8,
    },
}

impl PixelFormat {
    /// Number of bits per pixel, preserving unknown-format depth exactly.
    pub const fn bits_per_pixel(self) -> u8 {
        match self {
            Self::Mono1 => 1,
            Self::Gray2 => 2,
            Self::Gray4 => 4,
            Self::Gray8 => 8,
            Self::Rgb565 => 16,
            Self::Rgb888 => 24,
            Self::Other { bits, .. } => bits,
        }
    }

    /// Distinct gray levels, or None for colour and unknown formats.
    pub const fn gray_levels(self) -> Option<u16> {
        match self {
            Self::Mono1 => Some(2),
            Self::Gray2 => Some(4),
            Self::Gray4 => Some(16),
            Self::Gray8 => Some(256),
            Self::Rgb565 | Self::Rgb888 | Self::Other { .. } => None,
        }
    }

    fn wire(self) -> (u16, u16, u8) {
        let tag = match self {
            Self::Mono1 => @K_MONO1@,
            Self::Gray2 => @K_GRAY2@,
            Self::Gray4 => @K_GRAY4@,
            Self::Gray8 => @K_GRAY8@,
            Self::Rgb565 => @K_RGB565@,
            Self::Rgb888 => @K_RGB888@,
            Self::Other { code, bits } => return (@K_OTHER@, code, bits),
        };
        (tag, 0, self.bits_per_pixel())
    }
}

fn pixel_decode(kind: u16, code: u16, bits: u8) -> Result<PixelFormat, i32> {
    let pixel = match kind {
        @K_MONO1@ => PixelFormat::Mono1,
        @K_GRAY2@ => PixelFormat::Gray2,
        @K_GRAY4@ => PixelFormat::Gray4,
        @K_GRAY8@ => PixelFormat::Gray8,
        @K_RGB565@ => PixelFormat::Rgb565,
        @K_RGB888@ => PixelFormat::Rgb888,
        @K_OTHER@ => return Ok(PixelFormat::Other { code, bits }),
        _ => return Err(@ERR_UNSUPPORTED@),
    };
    if code != 0 || bits != pixel.bits_per_pixel() {
        return Err(@ERR_INVALID@);
    }
    Ok(pixel)
}

/// Encode exactly SNAPSHOT_BYTES, leaving every output byte unchanged on error.
///
/// Output may be uninitialized. Validation and capacity checks precede writes.
/// This is a status operation: a short output is ERR_NO_SPACE, never a prefix copy.
pub fn snapshot_encode(
    value: &Snapshot,
    out: &mut [core::mem::MaybeUninit<u8>],
) -> Result<usize, i32> {
    if value.battery.is_some_and(|battery| battery.percent > 100) {
        return Err(@ERR_INVALID@);
    }
    if out.len() < SNAPSHOT_BYTES {
        return Err(@ERR_NO_SPACE@);
    }
    let mut bytes = [0_u8; SNAPSHOT_BYTES];
    let mut presence = 0_u32;
    if let Some(battery) = value.battery {
        presence |= BATTERY;
        bytes[@PERCENT_OFFSET@] = battery.percent;
        bytes[@CHARGING_OFFSET@] = u8::from(battery.charging);
        bytes[@SAVER_OFFSET@] = u8::from(battery.saver);
        if let Some(voltage) = battery.millivolts {
            presence |= VOLTAGE;
            bytes[@MILLIVOLTS_OFFSET@..@MILLIVOLTS_END@].copy_from_slice(&voltage.to_le_bytes());
        }
    }
    if let Some(unix) = value.unix_seconds {
        presence |= UNIX;
        bytes[@UNIX_SECONDS_OFFSET@..@UNIX_SECONDS_END@].copy_from_slice(&unix.to_le_bytes());
    }
    bytes[@VERSION_OFFSET@..@VERSION_END@].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[@TOTAL_BYTES_OFFSET@..@TOTAL_BYTES_END@].copy_from_slice(&(SNAPSHOT_BYTES as u32).to_le_bytes());
    bytes[@PRESENCE_OFFSET@..@PRESENCE_END@].copy_from_slice(&presence.to_le_bytes());
    bytes[@MONOTONIC_MS_OFFSET@..@MONOTONIC_MS_END@].copy_from_slice(&value.monotonic_ms.to_le_bytes());
    bytes[@WIDTH_OFFSET@..@WIDTH_END@].copy_from_slice(&value.width.to_le_bytes());
    bytes[@HEIGHT_OFFSET@..@HEIGHT_END@].copy_from_slice(&value.height.to_le_bytes());
    bytes[@DPI_OFFSET@..@DPI_END@].copy_from_slice(&value.dpi.to_le_bytes());
    let (kind, code, bits) = value.pixel.wire();
    bytes[@PIXEL_KIND_OFFSET@..@PIXEL_KIND_END@].copy_from_slice(&kind.to_le_bytes());
    bytes[@PIXEL_CODE_OFFSET@..@PIXEL_CODE_END@].copy_from_slice(&code.to_le_bytes());
    bytes[@PIXEL_BITS_OFFSET@] = bits;
    bytes[@TOUCH_OFFSET@] = u8::from(value.touch);
    for (slot, byte) in out.iter_mut().zip(bytes) {
        slot.write(byte);
    }
    Ok(SNAPSHOT_BYTES)
}

fn scalar<const N: usize>(bytes: &[u8], offset: usize) -> [u8; N] {
    let mut value = [0; N];
    value.copy_from_slice(&bytes[offset..offset + N]);
    value
}

fn boolean(value: u8) -> Result<bool, i32> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(@ERR_INVALID@),
    }
}

/// Decode the entire bounded input before exposing its known prefix.
///
/// Same-version trailing extension bytes are accepted. The declared total must
/// equal the input length. Unknown versions, presence bits and pixel kinds are
/// ERR_UNSUPPORTED; malformed input is ERR_INVALID; over-cap input is ERR_NO_SPACE.
pub fn snapshot_decode(bytes: &[u8]) -> Result<Snapshot, i32> {
    if bytes.len() > SNAPSHOT_MAX_BYTES {
        return Err(@ERR_NO_SPACE@);
    }
    if bytes.len() < SNAPSHOT_BYTES {
        return Err(@ERR_INVALID@);
    }
    if u32::from_le_bytes(scalar(bytes, @VERSION_OFFSET@)) != SNAPSHOT_VERSION {
        return Err(@ERR_UNSUPPORTED@);
    }
    if u32::from_le_bytes(scalar(bytes, @TOTAL_BYTES_OFFSET@)) as usize != bytes.len() {
        return Err(@ERR_INVALID@);
    }
    let presence = u32::from_le_bytes(scalar(bytes, @PRESENCE_OFFSET@));
    if presence & !(BATTERY | VOLTAGE | UNIX) != 0 {
        return Err(@ERR_UNSUPPORTED@);
    }
    if presence & VOLTAGE != 0 && presence & BATTERY == 0 {
        return Err(@ERR_INVALID@);
    }
    if bytes[@RESERVED_OFFSET@..@RESERVED_END@].iter().any(|&byte| byte != 0) || bytes[@RESERVED_TAIL_OFFSET@..@RESERVED_TAIL_END@].iter().any(|&byte| byte != 0) {
        return Err(@ERR_INVALID@);
    }
    let unix = i64::from_le_bytes(scalar(bytes, @UNIX_SECONDS_OFFSET@));
    if presence & UNIX == 0 && unix != 0 {
        return Err(@ERR_INVALID@);
    }
    let percent = bytes[@PERCENT_OFFSET@];
    let millivolts = u16::from_le_bytes(scalar(bytes, @MILLIVOLTS_OFFSET@));
    let charging = boolean(bytes[@CHARGING_OFFSET@])?;
    let saver = boolean(bytes[@SAVER_OFFSET@])?;
    if percent > 100
        || (presence & VOLTAGE == 0 && millivolts != 0)
        || (presence & BATTERY == 0 && (percent != 0 || charging || saver))
    {
        return Err(@ERR_INVALID@);
    }
    Ok(Snapshot {
        monotonic_ms: u64::from_le_bytes(scalar(bytes, @MONOTONIC_MS_OFFSET@)),
        unix_seconds: if presence & UNIX == 0 {
            None
        } else {
            Some(unix)
        },
        battery: if presence & BATTERY == 0 {
            None
        } else {
            Some(Battery {
                percent,
                charging,
                saver,
                millivolts: if presence & VOLTAGE == 0 {
                    None
                } else {
                    Some(millivolts)
                },
            })
        },
        width: u16::from_le_bytes(scalar(bytes, @WIDTH_OFFSET@)),
        height: u16::from_le_bytes(scalar(bytes, @HEIGHT_OFFSET@)),
        dpi: u16::from_le_bytes(scalar(bytes, @DPI_OFFSET@)),
        touch: boolean(bytes[@TOUCH_OFFSET@])?,
        pixel: pixel_decode(
            u16::from_le_bytes(scalar(bytes, @PIXEL_KIND_OFFSET@)),
            u16::from_le_bytes(scalar(bytes, @PIXEL_CODE_OFFSET@)),
            bytes[@PIXEL_BITS_OFFSET@],
        )?,
    })
}
'''
