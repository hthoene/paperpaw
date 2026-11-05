"""Keep the inactive flat ABI under its own checked baseline, never unguarded.

The entire host_glue namespace is the interface-7 host boundary. Its text is
checked separately, not dropped; references/reexports into the old interface
are refused. rustdoc reachability covers aliases and public reexports too.
"""
import re

PREFIX = 'paperpaw_native::host_glue'
PATH = re.compile(r'(?<![A-Za-z0-9_:])paperpaw_[A-Za-z0-9_]+(?:::[A-Za-z_][A-Za-z0-9_]*)+')


def is_flat(path):
    return path == PREFIX or path.startswith(PREFIX + '::')


def owner(line):
    text = line.strip()
    if not text or text.startswith('# '):
        return None
    if text.startswith('pub use ') and ' as ' in text:
        text = text.split(' as ', 1)[1]
    if text.startswith('impl'):
        # A trait impl is owned by its self type, not the implemented trait.
        depth = 0
        for pos, char in enumerate(text):
            if char == '<':
                depth += 1
            elif char == '>' and not text[:pos].endswith('-'):
                depth = max(0, depth - 1)
            elif depth == 0 and text.startswith(' for ', pos):
                text = text[pos + 5:]
                break
        else:
            text = text[4:].lstrip()
            if text.startswith('<'):
                depth = 0
                for pos, char in enumerate(text):
                    depth += (char == '<') - (char == '>' and not text[:pos].endswith('-'))
                    if depth == 0:
                        text = text[pos + 1:]
                        break
    match = PATH.search(text)
    return match.group() if match else None


def partition(crate, text):
    legacy, flat = [], []
    for line in text.splitlines(keepends=True):
        defining = owner(line)
        if defining and is_flat(defining):
            if crate != 'paperpaw-native':
                raise ValueError('flat ABI definition outside paperpaw-native')
            flat.append(line)
        else:
            if any(is_flat(path) for path in PATH.findall(line)):
                raise ValueError(f'interface-6 item refers to interface-7: {line.strip()}')
            legacy.append(line)
    return ''.join(legacy), ''.join(flat)


def references(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key in ('resolved_path', 'trait', 'use', 'import') and isinstance(child, dict):
                if child.get('id') is not None:
                    yield str(child['id'])
            yield from references(child)
    elif isinstance(value, list):
        for child in value:
            yield from references(child)


def children(item):
    """Structural ownership only; a referenced type is not a child."""
    kind, data = next(iter(item['inner'].items()))
    if kind in ('module', 'trait', 'impl'):
        return data.get('items', [])
    if kind == 'enum':
        return data.get('variants', []) + data.get('impls', [])
    if kind in ('struct', 'variant'):
        form = data.get('kind')
        if isinstance(form, dict):
            if 'plain' in form:
                return form['plain'].get('fields', []) + data.get('impls', [])
            if 'struct' in form:
                return form['struct'].get('fields', [])
            return form.get('tuple', []) + data.get('impls', [])
        return data.get('impls', [])
    if kind == 'union':
        return data.get('fields', []) + data.get('impls', [])
    return []


def audit_rustdoc(doc):
    """Reject a public legacy root reaching flat declarations, even via aliases.

    Source ownership of reexports comes from the module tree, while target
    identity comes from rustdoc's canonical paths. Both are necessary: a renamed
    reexport must not disappear just because the target lives in host_glue.
    """
    index, paths = doc['index'], doc['paths']
    owners = {}
    stack = [(str(doc['root']), '')]
    while stack:
        ident, parent = stack.pop()
        if ident in owners or ident not in index:
            continue
        item = index[ident]
        name = item.get('name')
        own = '::'.join(part for part in (parent, name) if part)
        owners[ident] = own
        stack.extend((str(child), own) for child in children(item) if child is not None)

    def canonical(ident):
        return '::'.join(paths.get(ident, {}).get('path', []))

    def reaches_flat(start):
        todo, seen = list(references(start)), set()
        while todo:
            ident = todo.pop()
            if ident in seen:
                continue
            seen.add(ident)
            if is_flat(canonical(ident)):
                return canonical(ident)
            target = index.get(ident)
            if target:
                todo.extend(references(target['inner']))
                # Private fields/variants of a referenced type also cross.
                todo.extend(str(child) for child in children(target) if child is not None)
        return None

    for ident, item in index.items():
        if item.get('crate_id') != 0 or item.get('visibility') != 'public':
            continue
        own = owners.get(ident) or canonical(ident)
        if is_flat(own):
            continue
        reached = reaches_flat(item['inner'])
        if reached:
            raise ValueError(f'legacy export {own or ident} reaches flat ABI {reached}')
