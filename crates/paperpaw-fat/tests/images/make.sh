#!/bin/sh
# Makes the exFAT images of tests/exfat.rs anew, with exfatprogs'
# mkfs.exfat, the volume filled by exfat-fuse: another implementation's
# cards. Runs as root in Alpine Linux (fuse needs a privileged container),
# e.g. from this folder:
#
#   docker run --rm --privileged -v "$PWD:/img" alpine:3.20 sh /img/make.sh
#
# Each image is then kept sparse (`sparse.py`): only its blocks that are
# not all zeros.
set -e
apk add -q exfatprogs fuse-exfat util-linux python3
cd /img

# Empty, as sold: 8 MiB in clusters of 4 KiB, and 64 MiB in 32 KiB.
truncate -s 8M small.img
mkfs.exfat -q -L PAPERPAW small.img
truncate -s 64M big32k.img
mkfs.exfat -q -c 32K big32k.img

# A volume in a partition that starts at 1 MiB (an MBR, type 0x07).
truncate -s 16M part.img
mkfs.exfat -q part.img

# Filled by exfat-fuse: folders, long names outside ASCII, files of
# several clusters, a CrossPoint cache.
truncate -s 8M filled.img
mkfs.exfat -q filled.img
LOOP=$(losetup -f --show filled.img)
mkdir -p /mnt/x
mount.exfat-fuse "$LOOP" /mnt/x
mkdir -p '/mnt/x/books/Ünïcödé Földer'
printf 'hello' > /mnt/x/books/a.txt
seq 1 6000 > '/mnt/x/books/Ünïcödé Földer/a book with a very long name that goes on and on and on beyond eleven chars.epub'
seq 1 3000 > /mnt/x/big.bin
mkdir -p /mnt/x/.crosspoint/cache
for i in $(seq 1 40); do echo "$i" > "/mnt/x/.crosspoint/cache/f$i.bin"; done
sync
umount /mnt/x
losetup -d "$LOOP"

for f in small.img big32k.img part.img filled.img; do fsck.exfat -n "$f"; done
python3 sparse.py
rm -f small.img big32k.img part.img filled.img mbr.img
