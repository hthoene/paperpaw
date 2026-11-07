# Interface-7 menu replacement stack

The previous `CommandSender::set_menu` kept a 2,048-byte joining array on the
callback stack. A RISC-V release probe measured a 2,144-byte frame, above the
X4's 2,048-byte frame gate. The replacement uses a fallible allocation of exactly
the joined byte count, at most 2,048 bytes. Generated command encoding writes into
the caller's transport buffer; the temporary is freed before host submission.

| RISC-V probe | Previous implementation | Replacement |
| --- | ---: | ---: |
| `menu_stack_probe` frame | 2,144 B | 112 B |
| Visible static direct-call chain | 2,544 B | 448 B |

The baseline is `f0aa6444f`; its menu implementation is byte-for-byte identical
to the PR base `d7488aa58`. The replacement measured is `51f9eaedb`; subsequent
changes add tests and documentation. Both builds used Rust 1.99.0,
`riscv32imc-unknown-none-elf`, release optimization `s`, one codegen unit and no
LTO. `tools/stack-frames.py` read the resulting RISC-V ELFs. Static chains exclude
indirect calls and interrupt frames; packaging and runtime evidence remain
separate gates.

The compile-only [probe](../../tools/stack-probes/menu-set-menu.rs) preserves a
dynamic SDK call. Its allocator stubs refuse allocation; execution is unnecessary
for the scan. To reproduce from either checkout, run this Python from its root:

```python
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

repo = Path.cwd()
probe = Path(tempfile.mkdtemp(prefix="paperpaw-menu-stack-"))
(probe / "src").mkdir()
shutil.copy(repo / "tools/stack-probes/menu-set-menu.rs", probe / "src/main.rs")
sdk = json.dumps(str(repo / "crates/paperpaw-sdk"))
(probe / "Cargo.toml").write_text(f'''[package]
name="menu-stack-probe"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
paperpaw-sdk={{path={sdk}}}
[profile.release]
opt-level="s"
panic="abort"
lto=false
codegen-units=1
''')
target = probe / "target"
env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS="4")
subprocess.run(["cargo", "build", "--release", "--target",
                "riscv32imc-unknown-none-elf", "--manifest-path",
                str(probe / "Cargo.toml"), "-j4"], check=True, env=env)
elf = target / "riscv32imc-unknown-none-elf/release/menu-stack-probe"
subprocess.run(["python3", "tools/stack-frames.py", str(elf), "--from",
                "menu_stack_probe", "--top", "10", "--paths", "1"], check=True)
```

For the baseline checkout, copy the probe source from this change before running
that recipe. No SDK, ABI or generated-code changes are needed for the probe.

Allocator tests pin the maximum requested joining allocation to 2,048 bytes,
exercise allocation refusal, and prove empty/reset lists allocate nothing. A
combined menu-plus-handover test checks that the peak is the larger temporary,
not their sum, and that neither temporary survives into its host call. The X4
runtime heap measurement remains open.
