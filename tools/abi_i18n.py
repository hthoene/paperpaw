"""Validate and emit the i18n formatting limits (slots 27 and 28).

`docs/abi/screens-input.md` section 5: a format key, the encoded argument
batch and its record count are bounded, and so is the text `i18n.digits`
maps. Host and SDK read these limits from the generated constants, so they
cannot drift apart or borrow another family's cap.
"""


def _require(ok, message):
    if not ok:
        raise ValueError(message)


KEYS = {'since', 'max_key_bytes', 'max_args_bytes', 'max_args_records'}


def _entry(schema, name):
    entries = [item for item in schema.get('host_entries', [])
               if type(item) is dict and item.get('name') == name]
    _require(len(entries) == 1 and type(entries[0].get('max_bytes')) is int,
             f'i18n limits need the {name} entry')
    return entries[0]


def validate_i18n(schema):
    """The descriptor is closed; its caps fit the entries they bound."""
    descriptor = schema.get('i18n_limits')
    if descriptor is None:
        return
    _require(type(descriptor) is dict and set(descriptor) == KEYS, 'i18n limits keys')
    for key in KEYS:
        _require(type(descriptor[key]) is int and not isinstance(descriptor[key], bool),
                 f'i18n limits {key} must be an integer')
    _require(1 <= descriptor['since'] <= schema.get('minor', 0),
             'i18n limits since outside schema minor')
    format_entry = _entry(schema, 'i18n.format')
    _entry(schema, 'i18n.digits')
    _require(1 <= descriptor['max_key_bytes'] <= 255, 'i18n key cap')
    _require(1 <= descriptor['max_args_bytes'] <= format_entry['max_bytes'],
             'i18n argument byte cap')
    _require(1 <= descriptor['max_args_records'] <= 8, 'i18n argument record cap')


def compatible_i18n(old, new, minimum_since):
    """The limits are immutable once declared."""
    validate_i18n(old)
    validate_i18n(new)
    before, after = old.get('i18n_limits'), new.get('i18n_limits')
    if before is None:
        if after is not None:
            _require(after['since'] >= minimum_since, 'backdated i18n limits')
        return
    _require(after == before, 'changed or removed i18n limits')


def rust_i18n(schema):
    """Constants appended to the generated records codec; empty without limits."""
    validate_i18n(schema)
    descriptor = schema.get('i18n_limits')
    if descriptor is None:
        return ''
    digits = _entry(schema, 'i18n.digits')['max_bytes']
    return f'''
/// Maximum UTF-8 bytes of an `i18n.format` key (slot 27).
pub const I18N_FORMAT_MAX_KEY_BYTES: usize = {descriptor['max_key_bytes']};
/// Maximum bytes of an encoded `i18n.format` argument batch (slot 27).
pub const I18N_FORMAT_MAX_ARGUMENT_BYTES: usize = {descriptor['max_args_bytes']};
/// Maximum argument records in one `i18n.format` batch (slot 27).
pub const I18N_FORMAT_MAX_RECORDS: usize = {descriptor['max_args_records']};
/// Maximum UTF-8 bytes of the text `i18n.digits` maps (slot 28).
pub const I18N_DIGITS_MAX_BYTES: usize = {digits};
'''
