#!/usr/bin/env python3
r"""The heap's inventory of an X4 test image built with the feature
`heap-inventory` (#400, `shells/xteink/src/inventory.rs`): every live block
with its size and the code addresses found on the stack when it was made,
named from the image's ELF, summed by the first caller that is not the
allocator's own code.

  tools/heap-inventory.py dump --port rfc2217://esp-host:4000 --out inv.log
      (asks the running device for the inventory: the device must be settled
       where the figure is wanted, e.g. Home; the command is read like any
       remote command)
  tools/heap-inventory.py report inv.log --elf paperpaw-xteink [--top 40] [--depth 2]

The addresses come from a scan of the stack, not an unwinding: a stale word
can name a function that was not a caller. The sums are by requested size; the
device's `used` also counts what the allocator rounds up and its headers.
"""
import argparse
import bisect
import collections
import glob
import os
import re
import subprocess
import sys
import time

# The allocator's own frames and the containers' generic ones say nothing
# about who asked.
SKIP = re.compile(
    r"^(<?alloc::|<?core::|esp_alloc|__rust|<?hashbrown|<?&?mut |<?&?alloc|"
    r"_esp_alloc|critical_section|<?esp_alloc|linked_list|rust_oom|"
    r"<?std::)"
)


def dump(port, out, timeout):
    import serial

    link = serial.serial_for_url(port, do_not_open=True)
    link.timeout = 0.2
    link.dtr = False
    link.rts = False
    link.open()
    link.reset_input_buffer()
    link.write(b"inventory\n")
    deadline = time.monotonic() + timeout
    buf, lines = b"", []
    while time.monotonic() < deadline:
        buf += link.read(4096)
        while b"\n" in buf:
            line, buf = buf.split(b"\n", 1)
            text = line.decode(errors="replace").strip()
            if "PP:INV" in text:
                lines.append(text[text.index("PP:INV"):])
            if text.endswith("PP:END") and any("INV-END" in l for l in lines):
                open(out, "w").write("\n".join(lines) + "\n")
                print(f"{len(lines) - 1} blocks written to {out}")
                return
    sys.exit("no complete inventory before the timeout")


def symbols(elf):
    pattern = os.path.expanduser("~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-nm*")
    nm = next(iter(glob.glob(pattern)), "llvm-nm")
    out = subprocess.run(
        [nm, "--demangle", "--numeric-sort", "--print-size", "--defined-only", elf],
        capture_output=True, text=True, check=True,
    ).stdout
    starts, sizes, names = [], [], []
    for line in out.splitlines():
        parts = line.split(" ", 3)
        if len(parts) == 4 and parts[2] in "tTwW":
            starts.append(int(parts[0], 16))
            sizes.append(int(parts[1], 16))
            names.append(re.sub(r"::h[0-9a-f]{16}$", "", parts[3]))
    return starts, sizes, names


def report(log, elf, top, depth, show_chain):
    starts, sizes, names = symbols(elf)

    def name(addr):
        if not addr:
            return None
        i = bisect.bisect_right(starts, addr) - 1
        if i < 0 or addr >= starts[i] + max(sizes[i], 1):
            return f"?{addr:x}"
        return names[i]

    blocks, end = [], None
    for line in open(log):
        p = line.split()
        if len(p) > 1 and p[0] == "PP:INV":
            blocks.append((int(p[1], 16), int(p[2]), [int(x, 16) for x in p[3:7]]))
        elif p[:1] == ["PP:INV-END"]:
            end = dict(zip(p[1::2], p[2::2]))
    total = sum(b[1] for b in blocks)
    print(f"{len(blocks)} live blocks, {total} B requested")
    if end:
        used = int(end["used"])
        print(f"heap used {used} B (free {end['free']}); allocator rounding, headers and "
              f"untracked: {used - total} B; table dropped {end['dropped']} blocks / "
              f"{end['dropped_bytes']} B")
    sums = collections.defaultdict(lambda: [0, 0, collections.Counter()])
    for _, size, chain in blocks:
        names_ = [name(a) for a in chain if a]
        real = [n for n in names_ if not SKIP.match(n)]
        owner = " < ".join(real[:depth]) if real else "(" + (names_[0] if names_ else "no caller found") + ")"
        entry = sums[owner]
        entry[0] += size
        entry[1] += 1
        entry[2][size] += 1
    print(f"{'bytes':>8} {'blocks':>6}  caller (innermost first, allocator frames skipped)")
    for owner, (size, count, sizes_) in sorted(sums.items(), key=lambda kv: -kv[1][0])[:top]:
        common = ", ".join(f"{s}x{c}" for s, c in sizes_.most_common(3))
        print(f"{size:8} {count:6}  {owner}   [{common}]")
    if show_chain:
        for addr, size, chain in sorted(blocks, key=lambda b: -b[1])[:show_chain]:
            print(f"{size:8} " + " < ".join(str(name(a)) for a in chain))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    d = sub.add_parser("dump")
    d.add_argument("--port", required=True)
    d.add_argument("--out", required=True)
    d.add_argument("--timeout", type=float, default=120)
    r = sub.add_parser("report")
    r.add_argument("log")
    r.add_argument("--elf", required=True)
    r.add_argument("--top", type=int, default=40)
    r.add_argument("--depth", type=int, default=2)
    r.add_argument("--chains", type=int, default=0, help="also the chains of the largest blocks")
    a = ap.parse_args()
    if a.cmd == "dump":
        dump(a.port, a.out, a.timeout)
    else:
        report(a.log, a.elf, a.top, a.depth, a.chains)


if __name__ == "__main__":
    main()
