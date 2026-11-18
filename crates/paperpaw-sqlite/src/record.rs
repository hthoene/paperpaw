//! The record format: a header of serial types, then the values.

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::{Error, varint};

/// A value of a column.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Real(v) => Some(*v as i64),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(t) => Some(t),
            _ => None,
        }
    }
}

/// The serial type of an integer and its size in bytes.
fn int_type(v: i64) -> (u64, usize) {
    match v {
        0 => (8, 0),
        1 => (9, 0),
        -128..=127 => (1, 1),
        -32_768..=32_767 => (2, 2),
        -8_388_608..=8_388_607 => (3, 3),
        -2_147_483_648..=2_147_483_647 => (4, 4),
        -140_737_488_355_328..=140_737_488_355_327 => (5, 6),
        _ => (6, 8),
    }
}

/// A value's serial type and the bytes it takes in the body.
fn serial(value: &Value) -> (u64, usize) {
    match value {
        Value::Null => (0, 0),
        Value::Int(v) => int_type(*v),
        Value::Real(_) => (7, 8),
        Value::Text(t) => (t.len() as u64 * 2 + 13, t.len()),
        Value::Blob(b) => (b.len() as u64 * 2 + 12, b.len()),
    }
}

/// The header size of the record of `values` (it counts itself) and the
/// record's whole size; `None` past the address space.
fn record_size(values: &[Value]) -> Option<(usize, usize)> {
    let mut types = 0usize;
    let mut body = 0usize;
    for value in values {
        let (serial, size) = serial(value);
        types = types.checked_add(varint::len(serial))?;
        body = body.checked_add(size)?;
    }
    let mut size = types.checked_add(1)?;
    while varint::len(size as u64) + types != size {
        size = varint::len(size as u64).checked_add(types)?;
    }
    Some((size, size.checked_add(body)?))
}

/// Appends the record of `values`, whose header is `header` bytes, to
/// `out`, which has room for it.
fn write_record(values: &[Value], header: usize, out: &mut Vec<u8>) {
    varint::write(out, header as u64);
    for value in values {
        varint::write(out, serial(value).0);
    }
    for value in values {
        match value {
            Value::Null => {}
            Value::Int(v) => {
                let size = int_type(*v).1;
                out.extend_from_slice(&v.to_be_bytes()[8 - size..]);
            }
            Value::Real(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
            Value::Text(t) => out.extend_from_slice(t.as_bytes()),
            Value::Blob(b) => out.extend_from_slice(b),
        }
    }
}

/// `values` as a record.
pub fn encode_record(values: &[Value]) -> Vec<u8> {
    let (header, total) = record_size(values).unwrap_or((0, 0));
    let mut out = Vec::with_capacity(total);
    write_record(values, header, &mut out);
    out
}

/// [`encode_record`] with its one buffer allocated fallibly, sized exactly
/// before anything is written: [`Error::OutOfMemory`] instead of an abort.
///
/// # Errors
/// [`Error::OutOfMemory`] where the record's room cannot be had.
pub fn try_encode_record(values: &[Value]) -> Result<Vec<u8>, Error> {
    let (header, total) = record_size(values).ok_or(Error::OutOfMemory)?;
    let mut out = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| Error::OutOfMemory)?;
    write_record(values, header, &mut out);
    Ok(out)
}

/// The size of a value of serial type `serial`; `None` if it exceeds the
/// address space.
fn value_size(serial: u64) -> Option<usize> {
    match serial {
        0 | 8 | 9 | 10 | 11 => Some(0),
        1 => Some(1),
        2 => Some(2),
        3 => Some(3),
        4 => Some(4),
        5 => Some(6),
        6 | 7 => Some(8),
        n => usize::try_from((n - 12) / 2).ok(),
    }
}

fn read_int(bytes: &[u8]) -> i64 {
    // Sign-extended big-endian.
    let mut v: i64 = if bytes.first().is_some_and(|b| b & 0x80 != 0) {
        -1
    } else {
        0
    };
    for &b in bytes {
        v = (v << 8) | i64::from(b);
    }
    v
}

/// The values of a record; `None` if it is malformed (or does not fit in
/// memory).
pub fn decode_record(data: &[u8]) -> Option<Vec<Value>> {
    decode(data).ok()
}

/// The values of a record, allocated fallibly.
pub(crate) fn decode(data: &[u8]) -> Result<Vec<Value>, Error> {
    const MALFORMED: Error = Error::Corrupt("record");
    let (header_size, mut at) = varint::read(data).ok_or(MALFORMED)?;
    let header_size = usize::try_from(header_size).map_err(|_| MALFORMED)?;
    let mut body = header_size;
    let mut values = Vec::new();
    while at < header_size {
        let (serial, n) = data.get(at..).and_then(varint::read).ok_or(MALFORMED)?;
        at += n;
        let bytes = value_size(serial)
            .and_then(|size| data.get(body..body.checked_add(size)?))
            .ok_or(MALFORMED)?;
        body += bytes.len();
        let value = match serial {
            0 | 10 | 11 => Value::Null,
            1..=6 => Value::Int(read_int(bytes)),
            7 => Value::Real(f64::from_bits(read_int(bytes) as u64)),
            8 => Value::Int(0),
            9 => Value::Int(1),
            n if n % 2 == 1 => Value::Text(text(bytes)?),
            _ => Value::Blob(copy(bytes)?),
        };
        values.try_reserve(1).map_err(|_| Error::OutOfMemory)?;
        values.push(value);
    }
    Ok(values)
}

/// `bytes`, allocated fallibly.
fn copy(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(bytes.len())
        .map_err(|_| Error::OutOfMemory)?;
    copy.extend_from_slice(bytes);
    Ok(copy)
}

/// `bytes` as text, invalid UTF-8 replaced as by
/// [`String::from_utf8_lossy`].
fn text(bytes: &[u8]) -> Result<String, Error> {
    let mut text = String::new();
    for chunk in bytes.utf8_chunks() {
        let replacement = if chunk.invalid().is_empty() { 0 } else { 3 };
        text.try_reserve(chunk.valid().len() + replacement)
            .map_err(|_| Error::OutOfMemory)?;
        text.push_str(chunk.valid());
        if replacement > 0 {
            text.push(char::REPLACEMENT_CHARACTER);
        }
    }
    Ok(text)
}

/// SQLite's order of values with the BINARY collation: NULL, numbers,
/// text (bytewise), blobs.
fn compare_values(a: &Value, b: &Value) -> Ordering {
    let class = |v: &Value| match v {
        Value::Null => 0,
        Value::Int(_) | Value::Real(_) => 1,
        Value::Text(_) => 2,
        Value::Blob(_) => 3,
    };
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Int(x), Value::Real(y)) => compare_int_real(*x, *y),
        (Value::Real(x), Value::Int(y)) => compare_int_real(*y, *x).reverse(),
        (Value::Real(x), Value::Real(y)) => x.total_cmp(y),
        (Value::Text(x), Value::Text(y)) => x.as_bytes().cmp(y.as_bytes()),
        (Value::Blob(x), Value::Blob(y)) => x.cmp(y),
        _ => class(a).cmp(&class(b)),
    }
}

/// An integer and a real compared exactly, as SQLite does: not as two
/// reals, which are wrong beyond 2^53.
fn compare_int_real(int: i64, real: f64) -> Ordering {
    // 2^63: no `i64` reaches it.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if real.is_nan() {
        return (int as f64).total_cmp(&real);
    }
    if real < -LIMIT {
        return Ordering::Greater;
    }
    if real >= LIMIT {
        return Ordering::Less;
    }
    // The whole part of `real` fits an `i64`, and as a real again it is
    // exact: below 2^53 every whole number is, above it `real` is whole.
    let whole = real as i64;
    int.cmp(&whole)
        .then_with(|| (whole as f64).total_cmp(&real))
}

/// The order of two index entries (records), column by column.
pub fn compare_records(a: &[u8], b: &[u8]) -> Ordering {
    match (decode_record(a), decode_record(b)) {
        (Some(a), Some(b)) => {
            for (x, y) in a.iter().zip(b.iter()) {
                let order = compare_values(x, y);
                if order != Ordering::Equal {
                    return order;
                }
            }
            a.len().cmp(&b.len())
        }
        _ => a.cmp(b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn records_round_trip() {
        let values = vec![
            Value::Null,
            Value::Int(0),
            Value::Int(1),
            Value::Int(-5),
            Value::Int(300),
            Value::Int(70_000),
            Value::Int(1_760_000_000),
            Value::Int(1 << 40),
            Value::Int(i64::MIN),
            Value::Real(1.5),
            Value::Text("Fjord: ø".to_string()),
            Value::Blob(vec![1, 2, 3]),
        ];
        let data = encode_record(&values);
        assert_eq!(decode_record(&data), Some(values));
        // An empty record and a long header.
        assert_eq!(decode_record(&encode_record(&[])), Some(vec![]));
        let many = vec![Value::Int(7); 200];
        assert_eq!(decode_record(&encode_record(&many)), Some(many));
    }

    #[test]
    fn huge_serial_types_are_malformed() {
        // Blobs of 2^32 + 3 bytes (3 if cut to 32 bits) and of the most a
        // serial type can say.
        for serial in [((1u64 << 32) + 3) * 2 + 12, u64::MAX - 1] {
            let mut data = vec![0];
            varint::write(&mut data, serial);
            data[0] = data.len() as u8;
            data.extend_from_slice(&[1, 2, 3, 4, 5]);
            assert_eq!(decode_record(&data), None);
        }
    }

    #[test]
    fn records_sort_like_sqlite() {
        let r = |values: &[Value]| encode_record(values);
        let text = |t: &str| Value::Text(t.to_string());
        assert_eq!(
            compare_records(
                &r(&[Value::Int(2), Value::Int(9)]),
                &r(&[Value::Int(10), Value::Int(1)])
            ),
            Ordering::Less
        );
        assert_eq!(
            compare_records(&r(&[Value::Null]), &r(&[Value::Int(-9)])),
            Ordering::Less
        );
        assert_eq!(
            compare_records(&r(&[Value::Int(9)]), &r(&[text("0")])),
            Ordering::Less
        );
        assert_eq!(
            compare_records(&r(&[text("B")]), &r(&[text("a")])),
            Ordering::Less
        );
        assert_eq!(
            compare_records(
                &r(&[text("a"), Value::Int(2)]),
                &r(&[text("a"), Value::Int(2)])
            ),
            Ordering::Equal
        );
    }
}
