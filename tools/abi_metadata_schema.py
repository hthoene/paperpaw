"""Validation and resolved contract for the inactive metadata format draft."""
import re

from abi_layout import field_layout


def require(ok, message):
    if not ok:
        raise ValueError(message)


def keys(value, names):
    require(type(value) is dict and set(value) == set(names.split()), 'metadata keys: ' + names)


def integer(value, low, high):
    require(type(value) is int and low <= value <= high, 'metadata integer outside bounds')


def text(value, limit=128):
    require(type(value) is str and re.fullmatch(r'[A-Za-z0-9_.:/,+-]+', value) is not None,
            'invalid metadata identifier')
    require(len(value.encode('utf-8')) <= limit, 'metadata identifier too long')


def validate_metadata(schema):
    m = schema['metadata']
    keys(m, 'format byte_order magic max_bytes max_key_bytes max_services max_features '
         'max_exports containers profiles prefix exports services features records')
    integer(m['format'], 1, 2)
    require(m['byte_order'] == 'little' and m['magic'] == 'PPAM', 'unsupported metadata encoding')
    integer(m['max_bytes'], 32, 2048)
    integer(m['max_key_bytes'], 1, 128)
    integer(m['max_services'], 0, 16)
    integer(m['max_features'], 0, 32)
    export_count = 2 if m['format'] == 1 else 1
    integer(m['max_exports'], export_count, export_count)
    keys(m['containers'], 'elf macho pe wasm')
    for value in m['containers'].values():
        text(value)
    layout = field_layout(m['prefix'])
    require([x['name'] for x in layout['fields']] ==
            'magic layout prefix_bytes total_bytes abi_major needs_minor entry_layout '
            'entry_bytes flags target_bytes services features exports'.split(), 'metadata prefix fields')
    require([x['type'] for x in layout['fields']] ==
            'bytes4 u16 u16 u32 u16 u16 u16 u16 u32 u16 u16 u16 u16'.split(),
            'metadata prefix types')
    keys(m['records'], 'export service feature')
    fields = {'export': 'record_bytes kind reserved expected_bytes name_bytes',
              'service': 'record_bytes key_bytes major semantic_revision min_table_bytes',
              'feature': 'record_bytes key_bytes revision reserved'}
    types = {'export': 'u16 u8 u8 u32 u16',
             'service': 'u16 u16 u16 u16 u32', 'feature': 'u16 u16 u16 u16'}
    for kind, names in fields.items():
        record = field_layout(m['records'][kind])
        require([x['name'] for x in record['fields']] == names.split(), 'metadata record fields')
        require([x['type'] for x in record['fields']] == types[kind].split(), 'metadata record types')
    require(type(m['exports']) is list and len(m['exports']) == export_count, 'metadata export count')
    for item, kind, source in zip(m['exports'], (1, 2), ('entry_bytes', 'pointer_bytes')):
        keys(item, 'name kind size_source')
        text(item['name'])
        integer(item['kind'], kind, kind)
        require(item['size_source'] == source, 'metadata export size source')
    require(len({x['name'] for x in m['exports']}) == export_count, 'duplicate export')
    seen = set()
    require(type(m['profiles']) is list, 'profiles must be an array')
    for profile in m['profiles']:
        keys(profile, 'name pointer_bits byte_order calling_convention entry_layout entry_bytes entry_align')
        text(profile['name'], m['max_key_bytes'])
        text(profile['calling_convention'])
        require(profile['name'] not in seen, 'duplicate profile')
        seen.add(profile['name'])
        integer(profile['pointer_bits'], 32, 64)
        require(profile['pointer_bits'] in (32, 64) and profile['byte_order'] == 'little',
                'unsupported target representation')
        integer(profile['entry_layout'], 1, 65535)
        integer(profile['entry_bytes'], 8, 65535)
        integer(profile['entry_align'], 1, 16)
        align = profile['entry_align']
        require(align & (align - 1) == 0 and profile['entry_bytes'] % align == 0,
                'invalid entry alignment')
    for kind in ('services', 'features'):
        require(type(m[kind]) is list, 'requirement catalog must be an array')
        seen = set()
        for item in m[kind]:
            keys(item, 'key major semantic_revision min_table_bytes since' if kind == 'services'
                 else 'key revision since')
            text(item['key'], m['max_key_bytes'])
            require(item['key'] not in seen, 'duplicate requirement key')
            seen.add(item['key'])
            integer(item['since'], 1, schema['minor'])
            if kind == 'services':
                integer(item['major'], 1, 65535)
                integer(item['semantic_revision'], 1, 65535)
                integer(item['min_table_bytes'], 4, 2**32 - 1)
            else:
                integer(item['revision'], 1, 65535)


def resolved_metadata(schema):
    if 'metadata' not in schema:
        return None
    validate_metadata(schema)
    m = schema['metadata']
    result = dict(descriptor=m, prefix=field_layout(m['prefix']),
                  records={k: field_layout(v) for k, v in m['records'].items()})
    if 'entry_contract' in schema:
        result['locator'] = schema['entry_contract']['locator']
    return result


def compatible_metadata(old, new, minimum_since):
    before, after = old.get('metadata'), new.get('metadata')
    if before is None:
        if after is not None:
            require(new['minor'] >= minimum_since, 'backdated metadata format')
            for kind in ('services', 'features'):
                for item in after[kind]:
                    require(item['since'] >= minimum_since, 'backdated metadata requirement')
        return
    require(after is not None, 'removed metadata contract')
    catalogs = {'profiles', 'services', 'features'}
    compared = catalogs
    if before['format'] == 1 and after['format'] == 2:
        # Format 1 never had a production profile. Format 2 removes its
        # redundant global host-slot requirement; make carries the table.
        require(not before['profiles'], 'metadata format transition after target publication')
        require(new['minor'] >= minimum_since, 'backdated metadata format transition')
        require(after['exports'] == before['exports'][:1], 'changed entry export in format transition')
        compared = catalogs | {'format', 'exports', 'max_exports'}
    require({k: v for k, v in before.items() if k not in compared} ==
            {k: v for k, v in after.items() if k not in compared}, 'changed metadata encoding')
    for key in catalogs:
        prior, current = before[key], after[key]
        require(current[:len(prior)] == prior, 'changed/removed metadata declaration')
        if key != 'profiles':
            for item in current[len(prior):]:
                require(item['since'] >= minimum_since, 'backdated metadata requirement')
