"""A whole UI language as one data plugin (ADR 0033): the texts of every
plugin and of the kernel in one catalog, kept on the card compiled
(`i18n/<lang>.ppcat`) and read as they are needed.

The source of a language is one `.ini` catalog, `tools/plugin-files/languages/<lang>.ini`:
its `[meta]` and the messages of every plugin's `i18n/en.ini`, translated.
The source header holds the language metadata; data.toml holds its store
texts and dependencies. Adding a language needs no change to this tool.
This tool helps to make it and checks it:

    python3 tools/language-pack.py split OUT_DIR [N]
        The English messages of all plugins in N (8) files, OUT_DIR/chunk-NN.en.txt,
        a message a line (`key = English`) with the German beside it as a
        comment, for the translators.
    python3 tools/language-pack.py check LANG [FILE ...]
        What is wrong with the translations in FILE (default:
        tools/plugin-files/languages/LANG.ini): keys English does not have,
        placeholders that differ, empty messages, plural forms.
    python3 tools/language-pack.py merge LANG OUT.ini CHUNK ...
        The messages of the translated chunks, in the order of the English
        catalogs, with the language's [meta], as OUT.ini.
    python3 tools/language-pack.py build LANG OUT_DIR
        The data plugin `language-<lang>` in OUT_DIR: plugin.ini and
        i18n/<lang>.ppcat (made by paperpaw-sign), to copy into the device's
        plugins folder.

Messages English has and the translation lacks stay English (the kernel
falls back to it).
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LANGUAGES = ROOT / "tools/plugin-files/languages"

# English's plural forms, and the forms of Arabic's rule that a translation
# of such a pair may bring (a form it lacks reads as `other`).
PLURALS = ("zero", "one", "two", "few", "many", "other")
PLACEHOLDER = re.compile(r"\{[^{}]*\}")


def unquote(value):
    """A message's text: plain, or quoted with the escapes `\\n \\t \\\\ \\"`."""
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] == '"':
        return re.sub(r'\\(.)', lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)),
                      value[1:-1])
    return value


def quote(text):
    """The line's value for `text`: plain where it can be."""
    if text != text.strip() or "\n" in text or "\t" in text or text.startswith('"'):
        escaped = text.replace("\\", "\\\\").replace('"', '\\"')
        return '"' + escaped.replace("\n", "\\n").replace("\t", "\\t") + '"'
    return text


def parse(path, section=None):
    """`(meta, messages)` of the catalog in `path`: dicts, in file order. A
    translated chunk has no sections: its lines are `section`'s."""
    meta, messages = {}, {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("["):
            section = line.strip("[]")
            continue
        key, _, value = line.partition("=")
        target = {"meta": meta, "messages": messages}.get(section)
        if target is not None:
            target[key.strip()] = unquote(value)
    return meta, messages


def english():
    """Every English message of the plugins and the kernel, with its German
    where there is one, in a fixed order: `{key: (english, german)}`."""
    files = sorted(ROOT.glob("crates/*/i18n/en.ini")) + sorted(ROOT.glob("plugins/*/i18n/en.ini"))
    found = {}
    for file in files:
        german = file.with_name("de.ini")
        de = parse(german)[1] if german.exists() else {}
        for key, text in parse(file)[1].items():
            found.setdefault(key, (text, de.get(key, "")))
    return found


def split(out, n):
    messages = list(english().items())
    out = Path(out)
    out.mkdir(parents=True, exist_ok=True)
    size = -(-len(messages) // n)
    for i in range(n):
        lines = []
        for key, (en, de) in messages[i * size:(i + 1) * size]:
            lines.append(f"{key} = {quote(en)}")
            if de:
                lines.append(f"# de: {de}")
        (out / f"chunk-{i + 1:02}.en.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{len(messages)} messages in {n} files in {out}")


def forms(english_keys):
    """The keys a translation may have for English's: its own, and for a
    plural pair `x.one`/`x.other` every form of the rule."""
    allowed = set(english_keys)
    for key in english_keys:
        base, dot, form = key.rpartition(".")
        if dot and form in ("one", "other"):
            allowed.update(f"{base}.{p}" for p in PLURALS)
    return allowed


def check(lang, files):
    english_messages = english()
    allowed = forms(english_messages)
    problems = []
    seen = {}
    for file in files:
        _, messages = parse(file)
        for key, text in messages.items():
            if key in seen:
                problems.append(f"{file}: {key}: twice (also in {seen[key]})")
            seen[key] = file
            if key not in allowed:
                problems.append(f"{file}: {key}: not an English message")
                continue
            if not text.strip():
                problems.append(f"{file}: {key}: empty")
            # The placeholders of the English message of this key (a plural form of
            # the pair: that of `.other`, which has the count).
            base, _, form = key.rpartition(".")
            source = english_messages.get(key) or english_messages.get(f"{base}.other") or ("", "")
            want = sorted(PLACEHOLDER.findall(source[0]))
            got = sorted(PLACEHOLDER.findall(text))
            plural = form in PLURALS and f"{base}.other" in english_messages
            # A plural form may leave the count out ("one": "a plugin"), nothing else.
            if got != want and not (plural and set(got) <= set(want)):
                problems.append(f"{file}: {key}: placeholders {got} for {want}")
    missing = [k for k in english_messages if k not in seen]
    print(f"{len(seen)} messages, {len(missing)} of English's {len(english_messages)} not translated")
    return problems


def merge(lang, out, chunks):
    english_messages = english()
    translated = {}
    for chunk in chunks:
        translated.update(parse(chunk, "messages")[1])
    meta, _ = parse(LANGUAGES / f"{lang}.ini")
    if meta.get("language") != lang:
        raise ValueError(f"{lang}: set language = {lang} in the source catalog's [meta]")
    lines = ["[meta]"] + [f"{k} = {quote(v)}" for k, v in meta.items()]
    lines += ["", "[messages]"]
    done = set()
    for key in english_messages:
        base, dot, form = key.rpartition(".")
        if key in translated:
            lines.append(f"{key} = {quote(translated[key])}")
            done.add(key)
        if dot and form == "other":
            # The other forms of a plural pair, after the pair's last.
            for extra in PLURALS:
                k = f"{base}.{extra}"
                if k in translated and k not in done and k not in english_messages:
                    lines.append(f"{k} = {quote(translated[k])}")
                    done.add(k)
    Path(out).parent.mkdir(parents=True, exist_ok=True)
    Path(out).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{out}: {len(done)} messages")


def build(lang, base):
    """The data plugin of `lang` in `base`/language-<lang>: from its entry in
    data.toml (its version, dependencies, name and description)."""
    sys.path.insert(0, str(ROOT / "tools" / "plugin-files"))
    import data

    # Release builds start from lowercase plugin ids; source files keep
    # conventional language tags such as pt-BR.
    sources = [p for p in LANGUAGES.glob("*.ini") if p.stem.lower() == lang.lower()]
    if len(sources) != 1:
        raise ValueError(f"{lang}: expected exactly one matching language catalog")
    source = sources[0]
    lang = source.stem
    id = f"language-{lang.lower()}"
    out = Path(base) / id
    (out / "i18n").mkdir(parents=True, exist_ok=True)
    listed = data.plugins().get(id)
    if listed is None:
        sys.exit(f"{id}: add its entry to tools/plugin-files/data.toml before building")
    head = data.plugin_ini(id, listed)
    for path, text in data.catalogs(id, listed):
        (out / path).write_text(text, encoding="utf-8", newline="\n")
    (out / "plugin.ini").write_text(head, encoding="utf-8", newline="\n")
    subprocess.run(["cargo", "run", "-q", "--release", "-p", "paperpaw-sign", "--", "catalog",
                    str(source), str(out / "i18n" / f"{lang}.ppcat")], cwd=ROOT, check=True)
    print(f"{out}")


def main(args):
    match args:
        case [out] if not out.startswith("-") and out not in ("split", "check", "merge", "build"):
            # As the release's data.py runs a builder: the languages of data.toml.
            sys.path.insert(0, str(ROOT / "tools" / "plugin-files"))
            import data

            for id, fields in data.plugins().items():
                if fields["build"] == "tools/language-pack.py":
                    build(id.removeprefix("language-"), out)
        case ["split", out, *n]:
            split(out, int(n[0]) if n else 8)
        case ["check", lang, *files]:
            problems = check(lang, files or [LANGUAGES / f"{lang}.ini"])
            print("\n".join(problems[:200]))
            sys.exit(1 if problems else 0)
        case ["merge", lang, out, *chunks] if chunks:
            merge(lang, out, chunks)
        case ["build", lang, out]:
            build(lang, out)
        case _:
            sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
