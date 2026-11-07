# X4 device-test plugin (interface 7)

A tiny plugin for trying interface-7 plugin loading on a real Xteink X4
(feature `ppn5`, off by default; `abi/ppn5.md`). It registers `event.started`
(token 1) and counts the events it gets. Built three ways:

| file | what it does |
|---|---|
| `ok.ppn` | registers and counts; nothing visible, so look at the boot log (`native: <id>: format 5 made in … us, heap …`), Settings › Manage plugins and `PP:HEAP` |
| `crash-make.ppn` | faults (illegal instruction) while it is made: the device resets at every start until safe mode switches this plugin off and names it |
| `crash-event.ppn` | faults on its first `event.started` after the first frame: same, but the crash comes after the first frame |

```sh
export CARGO_TARGET_DIR=/own/target
samples/abi7-x4-device-test/build.sh        # -> dist/*.ppn and SHA256SUMS
```

`build.sh` needs the riscv32imc target and the packer (`paperpaw-sign pack`,
`abi/ppn5.md`); it also runs the packer's `plugin` check on each file. The
files are not signed. How a code plugin is put on the card (its folder with
`plugin.ini`, the installed hash in `.paperpaw/plugin-code.txt`, the author's
key) is the normal install path; see `docs/plugins.md`, "Plugins as files".
That path has not been tried with these files, and nothing here flashes or runs
a device.
