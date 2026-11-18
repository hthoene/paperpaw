#!/usr/bin/env bash
# Checks the plugin files a build made before they are signed, with the
# trusted checkout's tool (store-plugin.yml: the build is the branch's, the
# signing is not):
#
#   tools/plugin-files/check_files.sh <dir> <paperpaw-sign> <chip-id> none <file-name>
#
# <dir> holds <file-name> (<id>-<version>-<device>.ppn) alone, for the chip
# <chip-id> (5: the X4's, 9: the X4 Pro's and the X4 Classic's): anything else
# fails. The file is of interface 7 (format 5, abi/ppn5.md) and must say
# "format: 5" and the chip. The fourth word is `none`: such a file has no
# fingerprint, and files with one (the interface before 7) are no longer made.
set -euo pipefail

[[ $# -eq 5 ]] || { echo "usage: $0 <dir> <paperpaw-sign> <chip-id> none <file-name>" >&2; exit 2; }
dir=$1
sign=$2
chip=$3
fingerprint=$4
name=$5

[[ "$fingerprint" == none ]] || { echo "the fourth word is none: files with a fingerprint are no longer made" >&2; exit 1; }
[[ "$name" == */* ]] && { echo "$name: a file name, not a path" >&2; exit 2; }
shopt -s nullglob dotglob
found=("$dir"/*)
if [[ ${#found[@]} -ne 1 || "${found[0]}" != "$dir/$name" || ! -f "$dir/$name" || -L "$dir/$name" ]]; then
    echo "$dir: expected the file $name alone, found: ${found[*]:-nothing}" >&2
    exit 1
fi
shown="$("$sign" plugin "$dir/$name")"
if ! grep -Eq "^chip: .*\($chip\)\$" <<< "$shown"; then
    echo "$name: not for chip $chip" >&2
    echo "$shown" >&2
    exit 1
fi
if ! grep -Eq '^format: +5 ' <<< "$shown"; then
    echo "$name: not a file of interface 7 (format 5)" >&2
    echo "$shown" >&2
    exit 1
fi
echo "$name: chip $chip, format 5"
