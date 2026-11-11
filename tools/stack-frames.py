"""Prints the biggest stack frames of a firmware ELF and the deepest static
call chains through them, read from its disassembly: for checking the
X4's stack headroom without a device.

Each function's frame is what its prologue takes from the stack pointer
(RISC-V `addi sp, sp, -N` and `lui`/`sub` for big ones; Xtensa `entry a1,
N` and the `sub`/`movsp` after it). The call graph has the direct calls
and tail calls (which reuse the caller's place); calls through function
pointers and trait objects are not followed, so a chain through a
function marked `*` may go deeper at run time. Recursion is cut where it
closes, and chains into a panic are left out (`--panics` keeps them).
Interrupts come on top.

Usage:
    python tools/stack-frames.py ELF [--top 40] [--paths 12] [--min 512]
        [--root main] [--from REGEX ...] [--panics] [--objdump PATH]

    --top N      the N biggest frames
    --paths N    the deepest chain from the root, then the deepest
                 through other big frames under it: N chains in all
    --min BYTES  what a big frame is, for those chains
    --root NAME  where the chains start (default `main`)
    --cut RE     the functions whose names match RE are leaves: what they call
                 is not followed (for a path the caller knows is not taken)
    --from RE    also the deepest chains from the functions whose names
                 match RE (e.g. `Firmware>::run$`, or a plugin's `draw`)

The objdump is found in rustup's llvm-tools (RISC-V) or espup's
toolchain (Xtensa, `xtensa-esp32s3-elf-objdump`) unless given.

Example, the X4's release firmware:
    python tools/stack-frames.py \\
        shells/xteink/target/riscv32imc-unknown-none-elf/release/paperpaw-xteink
"""

import argparse
import glob
import re
import struct
import subprocess
import sys
from pathlib import Path

EM_RISCV = 243
EM_XTENSA = 94

HASH = re.compile(r"::h[0-9a-f]{16}$")
HEADER = re.compile(r"^([0-9a-f]+) <(.+)>:\s*$")
LINE = re.compile(r"^\s*([0-9a-f]+):\s+(\S+)\s*(.*)$")
ADDRESS = re.compile(r"\s*(?:0x)?([0-9a-f]+)")
# More than this in one step is not a frame (code of the radio's
# libraries that moves the stack pointer for other reasons).
FRAME_MAX = 0x10000

# Where a panic goes: the X4 restarts, so its chain is not one to fit.
PANIC = re.compile(
    r"^core::panicking::|^core::option::(unwrap|expect)_failed|^core::result::unwrap_failed"
    r"|^core::str::slice_error_fail|^core::slice::index::|slice_.*_fail|panic_bounds_check"
    r"|^__rustc::rust_begin_unwind|^alloc::alloc::handle_alloc_error|^alloc::raw_vec::handle_error"
    r"|^alloc::raw_vec::capacity_overflow|__unwrap_failed|^core::cell::panic_already"
)


def machine(elf):
    with open(elf, "rb") as f:
        head = f.read(20)
    if head[:4] != b"\x7fELF":
        sys.exit(f"{elf}: not an ELF file")
    return struct.unpack_from("<H", head, 18)[0]


class Image:
    """The bytes of an ELF's loaded sections, by address: for Xtensa's
    literals, which `l32r` loads."""

    def __init__(self, elf):
        data = Path(elf).read_bytes()
        (shoff,) = struct.unpack_from("<I", data, 32)
        shentsize, shnum = struct.unpack_from("<HH", data, 46)
        self.parts = []
        for i in range(shnum):
            _, kind, _, addr, offset, size = struct.unpack_from("<IIIIII", data, shoff + i * shentsize)
            if addr and kind == 1:  # SHT_PROGBITS
                self.parts.append((addr, data[offset:offset + size]))

    def word(self, addr):
        for start, part in self.parts:
            if start <= addr and addr + 4 <= start + len(part):
                return struct.unpack_from("<i", part, addr - start)[0]
        return None


def find_objdump(arch):
    home = Path.home()
    if arch == EM_RISCV:
        pattern = str(home / ".rustup/toolchains/*/lib/rustlib/*/bin/llvm-objdump*")
    else:
        pattern = str(home / ".rustup/toolchains/esp/xtensa-esp-elf/bin/xtensa-esp32s3-elf-objdump*")
    found = sorted(glob.glob(pattern))
    if not found:
        sys.exit("no objdump found: give one with --objdump")
    return found[-1]


def number(text):
    text = text.strip()
    neg = text.startswith("-")
    text = text.lstrip("-")
    value = int(text, 16) if text.startswith("0x") else int(text)
    return -value if neg else value


def address(text):
    m = ADDRESS.match(text)
    return int(m.group(1), 16) if m else None


def short(name):
    return HASH.sub("", name)


class Function:
    def __init__(self, name, addr):
        self.name = name
        self.addr = addr
        self.frame = 0
        self.calls = set()  # addresses
        self.tails = set()
        self.indirect = 0

    def take(self, n):
        if 0 < n < FRAME_MAX:
            self.frame += n


def parse(lines, arch, image):
    funcs, cur, regs = [], None, {}
    for raw in lines:
        m = HEADER.match(raw)
        if m:
            cur = Function(m.group(2), int(m.group(1), 16))
            funcs.append(cur)
            regs = {}
            continue
        m = LINE.match(raw) if cur else None
        if not m:
            continue
        pc, op = int(m.group(1), 16), m.group(2)
        args = m.group(3).split("#")[0].split("<")[0]
        ops = [a.strip() for a in args.split(",")] if args.strip() else []
        try:
            (riscv if arch == EM_RISCV else xtensa)(cur, pc, op, ops, regs, image)
        except ValueError:
            pass  # data in the code, read as instructions
    return funcs


def riscv(f, pc, op, ops, regs, _image):
    if len(ops) == 3 and ops[0] == "sp" and ops[1] == "sp":
        if op in ("addi", "c.addi16sp", "c.addi"):
            f.take(-number(ops[2]))
        elif op in ("sub", "c.sub"):
            f.take(regs.get(ops[2], 0))
        elif op == "add":
            f.take(-regs.get(ops[2], 0))
        return
    if op == "lui" and len(ops) == 2:
        v = (number(ops[1]) << 12) & 0xFFFFFFFF
        regs[ops[0]] = v - (1 << 32) if v & 0x80000000 else v
    elif op == "li" and len(ops) == 2:
        regs[ops[0]] = number(ops[1])
    elif op == "addi" and len(ops) == 3 and ops[1] in regs:
        regs[ops[0]] = regs[ops[1]] + number(ops[2])
    elif op == "auipc" and len(ops) == 2:
        regs["@" + ops[0]] = pc + ((number(ops[1]) << 12) & 0xFFFFFFFF)
        regs.pop(ops[0], None)
    elif op in ("jal", "c.jal"):
        # `jal 0x... <f>` or `jal ra, 0x... <f>`: a call; `jal zero`: a jump.
        a = address(ops[-1]) if ops else None
        if a is not None:
            (f.tails if len(ops) == 2 and ops[0] == "zero" else f.calls).add(a)
    elif op in ("j", "c.j"):
        a = address(ops[0]) if ops else None
        if a is not None:
            f.tails.add(a)  # within the function: dropped later
    elif op in ("jalr", "c.jalr", "jr", "c.jr"):
        # `jalr rs`, `jalr off(rs)`, `jalr rd, off(rs)`, `jr rs`.
        link = "zero" if op in ("jr", "c.jr") else "ra"
        if len(ops) == 2:
            link = ops[0]
        spec = ops[-1] if ops else ""
        m = re.match(r"(-?(?:0x)?[0-9a-f]+)?\((\w+)\)", spec)
        off, base = ((number(m.group(1)) if m.group(1) else 0), m.group(2)) if m else (0, spec)
        if "@" + base in regs:
            a = (regs["@" + base] + off) & 0xFFFFFFFF
            (f.calls if link != "zero" else f.tails).add(a)
        elif link != "zero":
            f.indirect += 1


def xtensa(f, pc, op, ops, regs, image):
    # The prologue: `entry a1, N` first, then for big frames the rest
    # taken by `sub aX, a1, aY` and `movsp a1, aX`. Only at the start: the
    # radio's libraries keep data among their code, which reads as
    # instructions too.
    regs["#"] = regs.get("#", 0) + 1
    prologue = regs["#"] <= 8
    if op == "entry" and len(ops) == 2 and ops[0] == "a1" and regs["#"] == 1:
        f.take(number(ops[1]))
    elif op in ("movi", "movi.n") and len(ops) == 2:
        regs[ops[0]] = number(ops[1])
    elif op == "l32r" and len(ops) == 2:
        a = address(ops[1])
        v = image.word(a) if a is not None else None
        if v is not None:
            regs[ops[0]] = v
    elif op == "sub" and len(ops) == 3 and prologue and ops[1] == "a1" and ops[2] in regs:
        if ops[0] == "a1":
            f.take(regs[ops[2]])
        else:
            regs["sp-" + ops[0]] = regs[ops[2]]
    elif op == "movsp" and len(ops) == 2 and prologue and ops[0] == "a1":
        f.take(regs.get("sp-" + ops[1], 0))
    elif op in ("call0", "call4", "call8", "call12"):
        a = address(ops[0]) if ops else None
        if a is not None:
            f.calls.add(a)
    elif op in ("callx0", "callx4", "callx8", "callx12"):
        # Far calls load the address from a literal (`l32r`) first.
        a = regs.get(ops[0]) if ops else None
        if a is not None:
            f.calls.add(a & 0xFFFFFFFF)
        else:
            f.indirect += 1
    elif op in ("j", "j.l"):
        a = address(ops[0]) if ops else None
        if a is not None:
            f.tails.add(a)
    elif ops and not op.startswith(XTENSA_READS_FIRST):
        regs.pop(ops[0], None)  # written by something not followed here


# Xtensa instructions whose first operand is read, not written.
XTENSA_READS_FIRST = ("s8", "s16", "s32", "b", "j", "call", "ret", "wsr", "wur", "loop", "entry")


def link(funcs, panics):
    """Turns call addresses into functions; drops jumps within one."""
    by_addr = {f.addr: f for f in funcs}
    for f in funcs:
        f.dead = not panics and bool(PANIC.search(short(f.name)))

    def to(addrs, f):
        found = (by_addr.get(a) for a in addrs)
        return {g for g in found if g is not None and g is not f and not g.dead}

    for f in funcs:
        f.tailees = to(f.tails, f)
        f.callees = to(f.calls, f) - f.tailees


def nexts(f):
    return sorted(f.callees | f.tailees, key=lambda g: g.addr)


def depths(funcs):
    """Each function's deepest static chain: its bytes and the next step.
    Depth first and without recursion in Python (the chains are long)."""
    depth, nxt, state = {}, {}, {}
    for start in funcs:
        if start in state:
            continue
        state[start] = 1
        stack = [(start, iter(nexts(start)))]
        while stack:
            f, it = stack[-1]
            g = next(it, None)
            if g is not None:
                if g not in state:
                    state[g] = 1
                    stack.append((g, iter(nexts(g))))
                continue
            best, step = f.frame, None
            for h in f.callees:
                if state[h] == 2 and f.frame + depth[h] > best:
                    best, step = f.frame + depth[h], h
            for h in f.tailees:
                if state[h] == 2 and depth[h] > best:
                    best, step = depth[h], h
            depth[f], nxt[f] = best, step
            state[f] = 2
            stack.pop()
    return depth, nxt


def entries(root):
    """How deep the stack is at most when each function under `root`
    starts, and the caller on that chain (recursion left out)."""
    order, seen, stack = [], {root}, [(root, iter(nexts(root)))]
    while stack:
        f, it = stack[-1]
        g = next(it, None)
        if g is None:
            order.append(f)
            stack.pop()
        elif g not in seen:
            seen.add(g)
            stack.append((g, iter(nexts(g))))
    order.reverse()
    pos = {f: i for i, f in enumerate(order)}
    entry, parent = {root: 0}, {root: None}
    for f in order:
        if f not in entry:
            continue
        for g, cost in [(g, f.frame) for g in f.callees] + [(g, 0) for g in f.tailees]:
            if pos[g] > pos[f] and entry[f] + cost > entry.get(g, -1):
                entry[g], parent[g] = entry[f] + cost, f
    return entry, parent


def chain(f, nxt):
    out = []
    while f is not None and f not in out:
        out.append(f)
        f = nxt.get(f)
    return out


def show(path):
    total = 0
    for i, f in enumerate(path):
        tail = i > 0 and f in path[i - 1].tailees
        if tail:
            total -= path[i - 1].frame
        total += f.frame
        mark = "*" if f.indirect else " "
        print(f"    {f.frame:6} {total:7} {mark} {'tail ' if tail else ''}{short(f.name)}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("elf")
    ap.add_argument("--top", type=int, default=40)
    ap.add_argument("--paths", type=int, default=12)
    ap.add_argument("--min", type=int, default=512)
    ap.add_argument("--root", default="main")
    ap.add_argument("--from", dest="froms", action="append", default=[])
    ap.add_argument("--panics", action="store_true")
    ap.add_argument("--cut", action="append", default=[],
                    help="REGEX: functions matching it are leaves (what they call is not followed)")
    ap.add_argument("--objdump")
    a = ap.parse_args()

    arch = machine(a.elf)
    if arch not in (EM_RISCV, EM_XTENSA):
        sys.exit(f"{a.elf}: neither RISC-V nor Xtensa")
    cmd = [a.objdump or find_objdump(arch), "-d", "-C", "--no-show-raw-insn", a.elf]
    out = subprocess.run(cmd, capture_output=True, text=True, errors="replace", check=True).stdout
    funcs = parse(out.splitlines(), arch, Image(a.elf) if arch == EM_XTENSA else None)
    link(funcs, a.panics)
    for pattern in a.cut:
        for f in funcs:
            if re.search(pattern, short(f.name)):
                f.callees, f.tailees = set(), set()
    depth, nxt = depths(funcs)

    print(f"{len(funcs)} functions. Columns: the frame's bytes, the chain's sum; "
          "* also calls through pointers\n")
    print(f"The {a.top} biggest frames (and the deepest chain from each):")
    for f in sorted(funcs, key=lambda f: -f.frame)[:a.top]:
        print(f"    {f.frame:6} {depth[f]:7} {'*' if f.indirect else ' '} {short(f.name)}")

    roots = [f for f in funcs if short(f.name) == a.root]
    if not roots:
        print(f"\nno function named {a.root}")
    for root in roots:
        print(f"\nThe deepest chain from {a.root}: {depth[root]} bytes")
        shown = chain(root, nxt)
        show(shown)
        # Then the deepest chains through other big frames under the root,
        # each through one that no chain before passed.
        entry, parent = entries(root)
        seen, count = set(shown), 1
        big = [g for g in entry if g.frame >= a.min]
        for g in sorted(big, key=lambda g: -(entry[g] + depth[g])):
            if count >= a.paths:
                break
            if g in seen:
                continue
            up, at = [], parent[g]
            while at is not None:
                up.append(at)
                at = parent[at]
            path = up[::-1] + chain(g, nxt)
            seen.update(path)
            count += 1
            print(f"\nThrough {short(g.name)}: {entry[g] + depth[g]} bytes")
            show(path)

    for pattern in a.froms:
        rx = re.compile(pattern)
        matched = sorted((f for f in funcs if rx.search(short(f.name))), key=lambda f: -depth[f])
        for f in matched[:a.paths]:
            print(f"\nThe deepest chain from {short(f.name)}: {depth[f]} bytes")
            show(chain(f, nxt))


if __name__ == "__main__":
    main()
