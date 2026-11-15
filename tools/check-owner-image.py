#!/usr/bin/env python3
"""Checks an X4 owner or release image before it leaves the team.

    check-owner-image.py IMAGE REFERENCE_FULL_IMAGE
    check-owner-image.py --release IMAGE

Every image handed to the owner is a FULL image (`espflash save-image --merge`),
because the owner flashes it at 0x0 and an app-only file would overwrite the
bootloader. It fails (exit 1) unless:

  - the bootloader's magic 0xE9 is at 0x0,
  - the partition table's magic AA 50 is at 0x8000 and the 0x1000 bytes of the
    table are byte-identical to the reference's (a Beta 3 full image: the layout
    must not change under an installed device),
  - the app image's magic 0xE9 is at 0x10000.

An app-only image (0xE9 at 0x0 with a long app, no table at 0x8000) is refused.
The bootloader's bytes are compared too, as a NOTE: a different bootloader is
allowed, but the report must say so. Prints OWNER_IMAGE_CHECK_EXIT=<0|1>.
Exit status: 0 ok, 1 refused, 2 usage.

`--release IMAGE` (the release workflow, on every image it builds) refuses an
image built as a test image (`test-image`, which also brings the never-sleep
defaults the device tests use): its high-water log lines are in no other build.
"""
import hashlib
import sys

TABLE_AT, TABLE_LEN, APP_AT = 0x8000, 0x1000, 0x10000
# Log strings only the test image has (`shells/xteink/src/high_water.rs`, compiled
# with `test-image`, whose defaults are also the never-sleep ones).
TEST_IMAGE_MARKS = (b"high-water: stack left", b"high-water: the run before")


def problems(img: bytes, ref: bytes) -> list[str]:
    bad = []
    if len(img) < APP_AT + 0x100:
        bad.append(f"image smaller than {APP_AT + 0x100:#x} bytes (app-only or truncated)")
    if img[:1] != b"\xe9":
        bad.append("no bootloader magic 0xE9 at 0x0")
    if img[TABLE_AT:TABLE_AT + 2] != b"\xaa\x50":
        bad.append("no partition table magic AA 50 at 0x8000")
    if img[APP_AT:APP_AT + 1] != b"\xe9":
        bad.append("no app image magic 0xE9 at 0x10000")
    if len(ref) < TABLE_AT + TABLE_LEN:
        bad.append("the reference is not a full image")
    elif img[TABLE_AT:TABLE_AT + TABLE_LEN] != ref[TABLE_AT:TABLE_AT + TABLE_LEN]:
        bad.append("partition table differs from the reference")
    return bad


def test_image_problems(img: bytes) -> list[str]:
    return [
        f"a test image (its log line {m.decode()!r} is in it): never a release"
        for m in TEST_IMAGE_MARKS
        if m in img
    ]


def bootloader_diff(img: bytes, ref: bytes) -> int:
    return sum(1 for x, y in zip(img[:TABLE_AT], ref[:TABLE_AT]) if x != y)


def release(path: str) -> int:
    with open(path, "rb") as f:
        img = f.read()
    bad = test_image_problems(img)
    for b in bad:
        print("FAIL:", b)
    print(f"RELEASE_IMAGE_CHECK_EXIT={1 if bad else 0}")
    return 1 if bad else 0


def main(argv: list[str]) -> int:
    if len(argv) == 2 and argv[0] == "--release":
        return release(argv[1])
    if len(argv) != 2:
        print(__doc__)
        return 2
    with open(argv[0], "rb") as f:
        img = f.read()
    with open(argv[1], "rb") as f:
        ref = f.read()
    print(f"size={len(img)} sha256={hashlib.sha256(img).hexdigest()[:16]} "
          f"bootloader bytes differing from reference: {bootloader_diff(img, ref)}")
    if bootloader_diff(img, ref):
        print("NOTE: bootloader differs from the reference; say so in the report")
    bad = problems(img, ref)
    for b in bad:
        print("FAIL:", b)
    print(f"OWNER_IMAGE_CHECK_EXIT={1 if bad else 0}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
