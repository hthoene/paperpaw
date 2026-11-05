"""Emit bounded raw plugin callback readers for schemas that declare them.

Readers retrieve unsafe function pointers only. They do not admit plugins,
validate plugin state, call callbacks or keep their owning modules alive.
"""
from abi_entry import validate_entry


_CALLBACKS = {
    'dispatch_event': [('state', 'pointer'), ('token', 'u32'), ('input', 'bytes')],
    'draw': [('state', 'pointer'), ('token', 'u32')],
}
_RUST_ARGS = {'pointer': '*mut core::ffi::c_void', 'u32': 'u32', 'bytes': 'PpBytes'}
_DROP = 'Option<unsafe extern "C" fn(*mut core::ffi::c_void)>'


def _callbacks(schema):
    validate_entry(schema)
    if 'entry_contract' not in schema:
        return []
    plugin = schema['entry_contract']['tables'][0]
    found = []
    for field in plugin['fields']:
        name = field['name']
        if name not in _CALLBACKS:
            continue
        expected = dict(args=[dict(name=name, type=kind)
                              for name, kind in _CALLBACKS[name]],
                        result='i32', nullable=True)
        if field.get('callback') != expected:
            raise ValueError(f'unsupported plugin callback signature: {name}')
        found.append(name)
    if found:
        statuses = schema.get('statuses')
        if type(statuses) is not list or any(type(item) is not dict for item in statuses):
            raise ValueError('plugin callback readers need status declarations')
        for name, value in (('ERR_UNSUPPORTED', -1), ('ERR_INVALID', -2)):
            matches = [item for item in statuses if item.get('name') == name]
            if (len(matches) != 1 or type(matches[0].get('value')) is not int
                    or matches[0]['value'] != value):
                raise ValueError(f'plugin callback readers need {name} = {value}')
    return found


def _reader(name):
    args = ', '.join(_RUST_ARGS[kind] for _, kind in _CALLBACKS[name])
    callback = f'unsafe extern "C" fn({args}) -> i32'
    return f'''/// Retrieve the declared `{name}` callback without calling it.
///
/// A missing tail field or null callback returns `ERR_UNSUPPORTED`. Malformed
/// alignment or byte counts return `ERR_INVALID`. Plugin state is not read.
///
/// # Safety
/// When non-null, aligned and within the checked length limits, `table` must
/// designate `accessible` readable bytes in one live allocation. The size and
/// callback fields read must be initialized, and the callback must have a valid
/// nullable function-pointer representation. Storage must not change during
/// this read. The caller must keep the callback's module loaded while using
/// the returned pointer and satisfy its call contract before invoking it.
pub unsafe fn read_plugin_{name}(
    table: *const u8,
    accessible: usize,
) -> Result<{callback}, i32> {{
    // SAFETY: The caller supplies readable stable storage and valid field values.
    let declared = unsafe {{ plugin_callback_size(table, accessible)? }};
    let offset = core::mem::offset_of!(PpPlugin, {name});
    let end = offset + core::mem::size_of::<Option<{callback}>>();
    if declared < end {{
        return Err(ERR_UNSUPPORTED);
    }}
    // SAFETY: Both lengths cover this aligned field; its representation is valid.
    let callback = unsafe {{ table.add(offset).cast::<Option<{callback}>>().read() }};
    callback.ok_or(ERR_UNSUPPORTED)
}}
'''


def rust_callbacks(schema):
    """Emit core-only readers, or nothing when neither known tail is present.

    The entry grammar validates field structure and its frozen prefix. Known
    callback names additionally require these exact nullable C signatures.
    No helper text is emitted for older schemas, preserving their lock output.
    """
    callbacks = _callbacks(schema)
    if not callbacks:
        return ''
    prefix = f'''// Check the size word before reading any optional callback field.
unsafe fn plugin_callback_size(table: *const u8, accessible: usize) -> Result<usize, i32> {{
    let size_offset = core::mem::offset_of!(PpPlugin, size);
    let size_end = size_offset + core::mem::size_of::<u32>();
    if table.is_null()
        || table.addr() % core::mem::align_of::<PpPlugin>() != 0
        || accessible < size_end
        || accessible > isize::MAX as usize
        || table.addr().checked_add(accessible).is_none()
    {{
        return Err(ERR_INVALID);
    }}
    // SAFETY: The checked span covers the aligned size word in caller-owned storage.
    let declared = unsafe {{ table.add(size_offset).cast::<u32>().read() }} as usize;
    let prefix_end = core::mem::offset_of!(PpPlugin, drop)
        + core::mem::size_of::<{_DROP}>();
    if declared < prefix_end || declared > accessible {{
        return Err(ERR_INVALID);
    }}
    Ok(declared)
}}
'''
    return prefix + '\n' + '\n'.join(_reader(name) for name in callbacks)
