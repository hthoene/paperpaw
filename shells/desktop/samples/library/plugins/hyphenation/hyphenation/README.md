# Hyphenation patterns

The pattern files are taken unchanged from the
[hyph-utf8](https://github.com/hyphenation/tex-hyphen) project
(`hyph-utf8/tex/generic/hyph-utf8/patterns/txt`). The build script compiles
them into tables for `paperpaw-hyphen`.

## English, American spelling (`hyph-en-us.pat.txt`, `hyph-en-us.hyp.txt`)

Hyphenation patterns for American English (`ushyphmax.tex`), version
2005-05-30, by Gerard D.C. Kuiken. Hyphen minimums: 2 characters before,
3 after.

```text
Copyright (C) 1990, 2004, 2005 Gerard D.C. Kuiken

Copying and distribution of this file, with or without modification,
are permitted in any medium without royalty provided the copyright
notice and this notice are preserved.
```

British English patterns are not included: their source notes that the
underlying word list may not be redistributed.

## Other languages

The hyphenation plugins of the store keep their own pattern files, with
source, authors, licence and hyphen minimums, in
`plugins/hyphenation-<language>/patterns/README.md`: German, French, Spanish,
Italian, Dutch, Portuguese, Swedish, Danish, Norwegian (Bokmål), Polish,
Russian and Ukrainian. They come from the same project.

Left out because their licence does not allow redistribution in an
Apache-2.0 project, or none is stated: Czech (GPL), Hungarian (MPL, GPL and
LGPL), Latvian (LGPL and GPL), Romanian (no licence stated), Finnish (only
"may be freely distributed", not clearly a licence for an Apache-2.0
project). Norwegian
Nynorsk is not included yet: its patterns equal Bokmål's, but its
exceptions differ.
