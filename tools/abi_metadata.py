"""Bounded draft ABI metadata codec for build tools.

This validates metadata structure, not artifact loadability, signatures, export
addresses, or a plugin's actual use of host functions. The build declaration
must list its uses honestly. A future loader must check compatibility itself.
The metadata schema is validated here; no target layouts are inferred here.
"""
import struct

from abi_layout import SCALARS, field_layout
from abi_metadata_schema import validate_metadata
from abi_requirements import required_minor


# Feature record flags bit 0: the plugin cannot run without the feature (D1).
FEATURE_REQUIRED = 1

def _require(condition, message):
    if not condition:
        raise ValueError(message)


def _integer(value, maximum, label, minimum=0):
    _require(type(value) is int and minimum <= value <= maximum,
             f'{label} must be an integer in {minimum}..{maximum}')
    return value


def _text(value, maximum, label):
    _require(type(value) is str, f'{label} must be text')
    try:
        encoded = value.encode('utf-8')
    except UnicodeEncodeError as error:
        raise ValueError(f'{label} is not UTF-8') from error
    _require(0 < len(encoded) <= maximum, f'{label} length outside 1..{maximum}')
    return encoded


def _wire(fields):
    layout = field_layout(fields)
    codec = struct.Struct('<' + ''.join(SCALARS[f['type']][0] for f in fields))
    _require(codec.size == layout['size'], 'wire layout size mismatch')
    return codec, [field['name'] for field in fields]


def _configuration(schema):
    _require(type(schema) is dict, 'schema must be an object')
    metadata = schema.get('metadata')
    _require(type(metadata) is dict, 'schema needs metadata')
    _integer(schema.get('major'), 65535, 'ABI major', 1)
    _integer(schema.get('minor'), 65535, 'ABI minor', 1)
    validate_metadata(schema)
    prefix = _wire(metadata['prefix'])
    records = {kind: _wire(metadata['records'][kind])
               for kind in ('export', 'service', 'feature')}
    profiles = _index(metadata.get('profiles'), 'name', metadata, 'profiles')
    catalogs = {kind: _index(metadata.get(kind), 'key', metadata, kind)
                for kind in ('services', 'features')}
    return metadata, prefix, records, profiles, catalogs


def _index(items, key, metadata, label):
    _require(type(items) is list, f'{label} must be a list')
    result = {}
    for item in items:
        _require(type(item) is dict, f'{label} item must be an object')
        name = item.get(key)
        _text(name, metadata['max_key_bytes'], f'{label} {key}')
        _require(name not in result, f'duplicate {label} {key}: {name}')
        result[name] = item
    return result


def _profile(profiles, target):
    _require(type(target) is str and target in profiles, f'unknown target: {target!r}')
    return profiles[target]


def _exports(metadata, profile):
    descriptors = metadata.get('exports')
    _require(type(descriptors) is list and len(descriptors) <= metadata['max_exports'],
             'invalid export descriptors')
    result, names, kinds = [], set(), set()
    for item in descriptors:
        _require(type(item) is dict, 'export descriptor must be an object')
        name, kind, source = item.get('name'), item.get('kind'), item.get('size_source')
        _text(name, metadata['max_key_bytes'], 'export name')
        _integer(kind, 255, 'export kind', 1)
        _require(name not in names and kind not in kinds, 'duplicate export descriptor')
        _require(type(source) is str and source in ('entry_bytes', 'pointer_bytes'),
                 'unknown export size source')
        expected = profile['entry_bytes'] if source == 'entry_bytes' else profile['pointer_bits'] // 8
        result.append({'name': name, 'kind': kind, 'expected_bytes': expected})
        names.add(name)
        kinds.add(kind)
    return result


def _select(metadata, catalogs, build, kind):
    values = build[kind]
    _require(type(values) is list and len(values) <= metadata['max_' + kind],
             f'{kind} count exceeds cap or is not a list')
    selected, seen = [], set()
    for value in values:
        # A feature may be selected as required: {'key': ..., 'required': True}.
        required = False
        if kind == 'features' and type(value) is dict:
            _require(set(value) == {'key', 'required'} and type(value['required']) is bool,
                     'feature selection needs key and required')
            value, required = value['key'], value['required']
        key = value
        _require(type(key) is str and key in catalogs[kind], f'unknown {kind} key: {key!r}')
        _require(key not in seen, f'duplicate {kind} key: {key}')
        seen.add(key)
        item = dict(catalogs[kind][key])
        if kind == 'features':
            item['flags'] = FEATURE_REQUIRED if required else 0
        selected.append(item)
    return sorted(selected, key=lambda item: item['key'])


def _pack(wire, values):
    codec, names = wire
    try:
        return codec.pack(*(values[name] for name in names))
    except (KeyError, struct.error) as error:
        raise ValueError(f'invalid wire values: {error}') from error


def _record(wire, values, text, length_name):
    values = dict(values, record_bytes=wire[0].size + len(text))
    values[length_name] = len(text)
    return _pack(wire, values) + text


def encode(schema, build):
    """Emit canonical bytes from explicit target, used, floor, services, features.

    The build cannot supply needs_minor, export sizes, or catalog revisions.
    Unknown targets fail, including when the schema has no verified profiles.
    Invalid inputs raise ValueError; neither input is changed.
    """
    metadata, prefix, records, profiles, catalogs = _configuration(schema)
    _require(type(build) is dict and set(build) ==
             {'target', 'used', 'floor', 'services', 'features'}, 'invalid build keys')
    profile = _profile(profiles, build['target'])
    needs_minor = required_minor(schema, build['used'], build['floor'])
    services = _select(metadata, catalogs, build, 'services')
    features = _select(metadata, catalogs, build, 'features')
    needs_minor = max([needs_minor] + [item['since'] for item in services + features])
    exports = _exports(metadata, profile)
    target = _text(build['target'], metadata['max_key_bytes'], 'target')
    total = prefix[0].size + len(target)
    for kind, items, key in [('export', exports, 'name'), ('service', services, 'key'),
                              ('feature', features, 'key')]:
        total += sum(records[kind][0].size + len(_text(item[key], metadata['max_key_bytes'], kind))
                     for item in items)
    _require(total <= metadata['max_bytes'], 'metadata exceeds byte cap')
    body = bytearray(target)
    for item in exports:
        body.extend(_record(records['export'], dict(item, reserved=0),
                            _text(item['name'], metadata['max_key_bytes'], 'export'), 'name_bytes'))
    for kind, items in [('service', services), ('feature', features)]:
        for item in items:
            body.extend(_record(records[kind], item,
                                _text(item['key'], metadata['max_key_bytes'], kind), 'key_bytes'))
    values = dict(magic=metadata['magic'].encode('utf-8'), layout=metadata['format'],
                  prefix_bytes=prefix[0].size, total_bytes=total, abi_major=schema['major'],
                  needs_minor=needs_minor, entry_layout=profile['entry_layout'],
                  entry_bytes=profile['entry_bytes'], flags=0, target_bytes=len(target),
                  services=len(services), features=len(features), exports=len(exports))
    return _pack(prefix, values) + body


class _Reader:
    def __init__(self, data):
        self.data = data
        self.position = 0

    def take(self, count):
        _require(0 <= count <= len(self.data) - self.position, 'truncated metadata')
        result = self.data[self.position:self.position + count]
        self.position += count
        return result

    def unpack(self, wire):
        return dict(zip(wire[1], wire[0].unpack(self.take(wire[0].size))))

    def text(self, count, maximum):
        _require(0 < count <= maximum, 'text length outside cap')
        try:
            return self.take(count).decode('utf-8')
        except UnicodeDecodeError as error:
            raise ValueError('invalid UTF-8 metadata text') from error

    def record(self, wire, length_name, maximum):
        values = self.unpack(wire)
        length = values[length_name]
        _require(values['record_bytes'] == wire[0].size + length, 'invalid record length')
        return values, self.text(length, maximum)


def _decode_catalog(reader, wire, count, catalog, metadata, kind):
    """Decode service or feature requirements in canonical order.

    A feature's flags bit 0 means required (D1, #890). Feature revisions only
    add, so the host serves any requested revision up to its own. A feature this
    host lacks, or has only at a lower revision than requested, is unavailable:
    an optional one is left out of the result, a required one refuses the load
    and names the key. Other flag bits are
    refused. Services stay closed: an unknown service key is always refused.
    """
    result, seen, previous = [], set(), None
    for _ in range(count):
        values, key = reader.record(wire, 'key_bytes', metadata['max_key_bytes'])
        _require(key not in seen, f'duplicate {kind} key: {key}')
        _require(previous is None or previous < key, f'noncanonical {kind} order')
        seen.add(key)
        previous = key
        if kind == 'feature':
            _require(values['flags'] & ~FEATURE_REQUIRED == 0, 'unknown feature flag bits')
            if key not in catalog or values['revision'] > catalog[key]['revision']:
                _require(not values['flags'] & FEATURE_REQUIRED,
                         f'required feature unavailable on this host: {key}')
                continue
        _require(key in catalog, f'unknown {kind} key: {key!r}')
        fields = ('major', 'semantic_revision', 'min_table_bytes') if kind == 'service' else ('revision',)
        # Services match exactly; a feature's revision was checked above (requested <= host).
        _require(kind == 'feature' or all(values[field] == catalog[key][field] for field in fields),
                 f'{kind} requirement differs from catalog')
        item = dict(key=key, **{field: values[field] for field in fields})
        if kind == 'feature':
            item['required'] = bool(values['flags'] & FEATURE_REQUIRED)
        result.append(item)
    return result


def decode(schema, data):
    """Validate a complete bounded block and return JSON-serializable fields.

    A needs_minor above the local minor is structurally valid. This function
    never claims the plugin is loadable and cannot reconstruct used or floor.
    """
    metadata, prefix, records, profiles, catalogs = _configuration(schema)
    _require(type(data) is bytes, 'metadata must be bytes')
    _require(prefix[0].size <= len(data) <= metadata['max_bytes'], 'metadata size outside cap')
    reader = _Reader(data)
    values = reader.unpack(prefix)
    _require(values['magic'] == metadata['magic'].encode('utf-8'), 'invalid metadata magic')
    _require(values['layout'] == metadata['format'], 'unknown metadata layout')
    _require(values['prefix_bytes'] == prefix[0].size, 'invalid prefix byte count')
    _require(values['total_bytes'] == len(data), 'invalid total byte count')
    _require(values['abi_major'] == schema['major'], 'unknown ABI major')
    _require(values['flags'] == 0, 'nonzero metadata flags')
    _require(values['needs_minor'] >= 1, 'invalid required minor')
    for kind in ('exports', 'services', 'features'):
        _require(values[kind] <= metadata['max_' + kind], f'{kind} count exceeds cap')
    minimum = values['target_bytes'] + sum(values[kind + 's'] * (records[kind][0].size + 1)
                                          for kind in ('export', 'service', 'feature'))
    _require(minimum <= len(data) - reader.position, 'record counts exceed metadata length')
    target = reader.text(values['target_bytes'], metadata['max_key_bytes'])
    profile = _profile(profiles, target)
    _require(values['entry_layout'] == profile['entry_layout'] and
             values['entry_bytes'] == profile['entry_bytes'], 'entry layout differs from profile')
    expected = _exports(metadata, profile)
    _require(values['exports'] == len(expected), 'invalid export count')
    exports = []
    for wanted in expected:
        item, name = reader.record(records['export'], 'name_bytes', metadata['max_key_bytes'])
        _require(item['reserved'] == 0, 'nonzero export reserved field')
        actual = dict(name=name, kind=item['kind'], expected_bytes=item['expected_bytes'])
        _require(actual == wanted, 'export differs from target profile')
        exports.append(actual)
    services = _decode_catalog(reader, records['service'], values['services'],
                               catalogs['services'], metadata, 'service')
    features = _decode_catalog(reader, records['feature'], values['features'],
                               catalogs['features'], metadata, 'feature')
    since = [catalogs[kind][item['key']]['since']
             for kind, items in [('services', services), ('features', features)] for item in items]
    _require(all(values['needs_minor'] >= minor for minor in since), 'required minor below catalog since')
    _require(reader.position == len(data), 'trailing metadata bytes')
    return dict(layout=values['layout'], prefix_bytes=values['prefix_bytes'],
                total_bytes=values['total_bytes'], abi_major=values['abi_major'],
                needs_minor=values['needs_minor'], flags=values['flags'], target=target,
                profile=dict(profile), entry=dict(layout=profile['entry_layout'],
                bytes=profile['entry_bytes'], align=profile['entry_align']),
                exports=exports, services=services, features=features)
