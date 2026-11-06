"""Aggregate draft ABI minor requirements before emitting plugin metadata.

The caller must validate the full schema with tools/abi-gen.py and combine
service and feature dependencies separately before emitting metadata. This
helper resolves only explicit host entries, records and fields. It does not
activate the draft ABI or infer which declarations a plugin uses.
"""


def _integer(value, label, maximum, minimum=0):
    if type(value) is not int or not minimum <= value <= maximum:
        raise ValueError(f'{label} must be an integer in {minimum}..{maximum}')
    return value


def _index(items, label, minor):
    if type(items) is not list:
        raise ValueError(f'{label} must be a list')
    result = {}
    for item in items:
        if type(item) is not dict:
            raise ValueError(f'{label} declaration must be an object')
        name = item.get('name')
        if type(name) is not str or not name:
            raise ValueError(f'{label} declaration needs a name')
        if name in result:
            raise ValueError(f'duplicate {label} name: {name}')
        _integer(item.get('since'), f'{label} {name} since', minor, 1)
        result[name] = item
    return result


def _lookup(index, name, label):
    if type(name) is not str or name not in index:
        raise ValueError(f'unknown {label}: {name!r}')
    return index[name]


def required_minor(schema, used, floor):
    """Return the maximum floor and referenced declaration ``since`` values.

    ``used`` must be a list of exact objects: ``{kind: 'host', name: ...}``,
    ``{kind: 'record', name: ...}``, ``{kind: 'callback', name: ...}``, or
    ``{kind: 'field', record: ..., name: ...}``. Names match the schema
    exactly. A field also requires its enclosing record's minor. A record
    requirement alone does not require every field added later.

    An empty list is conservative and requires ``floor == schema['minor']``.
    An explicit list may use floor zero. Callers cannot override a reference
    with a supplied minor; unknown, malformed or duplicate requirements fail.
    Inputs are read without mutation. Invalid inputs raise ``ValueError``.
    """
    if type(schema) is not dict:
        raise ValueError('schema must be an object')
    minor = _integer(schema.get('minor'), 'schema minor', 65535, 1)
    _integer(floor, 'floor', minor)
    if type(used) is not list:
        raise ValueError('used must be a list')
    if not used and floor != minor:
        raise ValueError('empty used requires the current schema minor as floor')
    hosts = _index(schema.get('host_entries'), 'host', minor)
    records = _index(schema.get('records'), 'record', minor)
    fields = {name: _index(record.get('fields'), f'field in {name}', minor)
              for name, record in records.items()}
    shapes = {'host': {'kind', 'name'}, 'record': {'kind', 'name'},
              'callback': {'kind', 'name'},
              'field': {'kind', 'record', 'name'}}
    seen = set()
    required = floor
    for item in used:
        if type(item) is not dict:
            raise ValueError('requirement must be an object')
        kind = item.get('kind')
        if type(kind) is not str or kind not in shapes:
            raise ValueError(f'unknown requirement kind: {kind!r}')
        if set(item) != shapes[kind]:
            raise ValueError(f'{kind} requirement needs exactly {sorted(shapes[kind])}')
        if kind == 'callback':
            contract = schema.get('entry_contract')
            if type(contract) is not dict:
                raise ValueError('callback needs entry contract')
            tables = _index(contract.get('tables'), 'entry table', minor)
            table = _lookup(tables, 'PpPlugin', 'entry table')
            callbacks = _index(table.get('fields'), 'plugin callback', minor)
            declaration = _lookup(callbacks, item['name'], 'plugin callback')
            if 'callback' not in declaration:
                raise ValueError('requirement does not name a callback')
            required = max(required, table['since'],
                           _integer(contract.get('since'), 'entry since', minor, 1))
            identity = (kind, item['name'])
        elif kind == 'field':
            record = _lookup(records, item['record'], 'record')
            declaration = _lookup(fields[item['record']], item['name'], 'field')
            required = max(required, record['since'])
            identity = (kind, item['record'], item['name'])
        else:
            index = hosts if kind == 'host' else records
            declaration = _lookup(index, item['name'], kind)
            identity = (kind, item['name'])
        if identity in seen:
            raise ValueError(f'duplicate requirement: {identity!r}')
        seen.add(identity)
        required = max(required, declaration['since'])
        if kind == 'host' and item['name'] == 'settings.read' and 'settings_value' in schema:
            # Slot 5 consumes this codec even though its C signature is older.
            required = max(required, _integer(schema['settings_value'].get('since'),
                                              'settings value since', minor, 1))
        if kind == 'host' and item['name'] == 'env.snapshot' and 'snapshot_value' in schema:
            # Slot 4's stable signature still requires its selected wire layout.
            required = max(required, _integer(schema['snapshot_value'].get('since'),
                                              'snapshot value since', minor, 1))
    return required
