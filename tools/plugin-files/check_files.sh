#!/usr/bin/env bash
# Checks the plugin files a build made before they are signed, with the
# trusted checkout's tool (store-plugin.yml: the build is the branch's, the
# signing is not):
#
#   tools/plugin-files/check_files.sh <dir> <paperpaw-sign> <chip-id> <fingerprint> <file-name>
#
# <dir> holds <file-name> (<id>-<version>-<device>.ppn) alone, for the chip
# <chip-id> (5: the X4's, 9: the X4 Pro's and the X4 Classic's), with the
# firmware's <fingerprint> (the base release's): anything else fails. A file of
# interface 7 (format 5, abi/ppn5.md) has no fingerprint: with `none` it must say
# "format: 5" and the chip.
set -euo pipefail

[[ $# -eq 5 ]] || { echo "usage: $0 <dir> <paperpaw-sign> <chip-id> <fingerprint> <file-name>" >&2; exit 2; }
dir=$1
sign=$2
chip=$3
fingerprint=$4
name=$5

[[ -n "$fingerprint" ]] || { echo "no fingerprint to check against" >&2; exit 1; }
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
if [[ "$fingerprint" == none ]]; then
    if ! grep -Eq '^format: +5 ' <<< "$shown"; then
        echo "$name: not a file of interface 7 (format 5)" >&2
        echo "$shown" >&2
        exit 1
    fi
    echo "$name: chip $chip, format 5"
    exit 0
fi
if ! grep -Eq "^fingerprint: +$fingerprint\$" <<< "$shown"; then
    echo "$name does not fit the firmware (fingerprint $fingerprint)" >&2
    echo "$shown" >&2
    exit 1
fi
echo "$name: chip $chip, fingerprint $fingerprint"
