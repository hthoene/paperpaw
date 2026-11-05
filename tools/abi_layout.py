"""Fixed little-endian metadata field layouts shared by generator and codec."""

SCALARS = {'u8': ('B', 1), 'u16': ('H', 2), 'u32': ('I', 4), 'bytes4': ('4s', 4)}


def field_layout(fields):
    if type(fields) is not list:
        raise ValueError('layout fields must be an array')
    offset, names, resolved = 0, set(), []
    for field in fields:
        if type(field) is not dict or set(field) != {'name', 'type'}:
            raise ValueError('invalid layout field')
        name, kind = field['name'], field['type']
        if type(name) is not str or not name.isascii() or not name.isidentifier():
            raise ValueError('invalid layout field name')
        if type(kind) is not str or kind not in SCALARS or name in names:
            raise ValueError('unknown or duplicate layout field')
        names.add(name)
        width = SCALARS[kind][1]
        resolved.append(dict(name=name, type=kind, offset=offset, width=width))
        offset += width
    return dict(size=offset, fields=resolved)
