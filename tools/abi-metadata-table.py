#!/usr/bin/env python3
"""The table `paperpaw-dev metadata` makes a plugin's ABI metadata from.

`tools/abi-metadata.py` makes `native/metadata.bin` from the schema
(`abi/abi.json`), which only this repository has. An author's `paperpaw-dev`
carries what that needs as one text file, `tools/dev/src/metadata/table.txt`:
the metadata's layouts and caps, the target profiles, the service and feature
catalogs, and for every host function, callback, record and field the minor it
needs (`abi_requirements.required_minor` of that one requirement, so the rules
live here alone).

    tools/abi-metadata-table.py           # writes the table
    tools/abi-metadata-table.py --check   # fails if it is not current
"""
import json
from pathlib import Path
import sys

from abi_metadata import _configuration
from abi_requirements import required_minor

ROOT = Path(__file__).resolve().parent.parent
TABLE = ROOT / 'tools/dev/src/metadata/table.txt'


def pairs(item, keys):
    return ' '.join(f'{key}={item[key]}' for key in keys)


def layout(fields):
    return ' '.join(f"{field['name']}:{field['type']}" for field in fields)


def table(schema):
    metadata = _configuration(schema)[0]
    lines = ['# Written by tools/abi-metadata-table.py from abi/abi.json; not by hand.',
             f"abi {schema['major']} {schema['minor']}",
             'metadata ' + pairs(metadata, ('magic', 'format', 'max_bytes', 'max_key_bytes',
                                            'max_exports', 'max_services', 'max_features')),
             'layout prefix ' + layout(metadata['prefix'])]
    lines += [f'layout {kind} ' + layout(metadata['records'][kind])
              for kind in ('export', 'service', 'feature')]
    lines += [f"profile {item['name']} " + pairs(item, ('entry_layout', 'entry_bytes', 'pointer_bits'))
              for item in metadata['profiles']]
    lines += [f"export {item['name']} " + pairs(item, ('kind', 'size_source'))
              for item in metadata['exports']]
    lines += [f"service {item['key']} " + pairs(item, ('since', 'major', 'semantic_revision',
                                                       'min_table_bytes'))
              for item in metadata['services']]
    lines += [f"feature {item['key']} " + pairs(item, ('since', 'revision'))
              for item in metadata['features']]

    def minor(requirement):
        return required_minor(schema, [requirement], 0)

    lines += [f"host {item['name']} {minor(dict(kind='host', name=item['name']))}"
              for item in schema['host_entries']]
    plugin = next(t for t in schema['entry_contract']['tables'] if t['name'] == 'PpPlugin')
    lines += [f"callback {item['name']} {minor(dict(kind='callback', name=item['name']))}"
              for item in plugin['fields'] if 'callback' in item]
    for record in schema['records']:
        lines.append(f"record {record['name']} {minor(dict(kind='record', name=record['name']))}")
        lines += [f"field {record['name']} {item['name']} "
                  f"{minor(dict(kind='field', record=record['name'], name=item['name']))}"
                  for item in record['fields']]
    for line in lines[1:]:
        if not line.isascii() or '  ' in line or line != line.strip():
            raise ValueError(f'a name does not fit the table: {line!r}')
    return '\n'.join(lines) + '\n'


def main(argv):
    if any(arg != '--check' for arg in argv):
        print(__doc__, file=sys.stderr)
        return 2
    text = table(json.loads((ROOT / 'abi/abi.json').read_text(encoding='utf-8')))
    if '--check' in argv:
        if not TABLE.is_file() or TABLE.read_text(encoding='utf-8') != text:
            print(f'{TABLE.relative_to(ROOT)} is not current: run tools/abi-metadata-table.py',
                  file=sys.stderr)
            return 1
        return 0
    TABLE.parent.mkdir(parents=True, exist_ok=True)
    TABLE.write_text(text, encoding='utf-8', newline='\n')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
