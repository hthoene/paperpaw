# Architecture Decision Records

Significant, hard-to-reverse decisions are recorded here, one file each,
numbered in order. A decision is never edited after acceptance. Its
status line may say what changed since; a change of the decision itself
is a new record that links back.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-rust-no-std-core.md) | Rust with a `no_std` core | Accepted; its WebAssembly plugins became native code plugins ([0024](0024-native-code-plugins.md)) |
| [0002](0002-apache-2-license.md) | Apache License 2.0 | Accepted |
| [0003](0003-sans-io-kernel.md) | Sans-I/O kernel | Accepted; its WebAssembly plugins became native code plugins ([0024](0024-native-code-plugins.md)) |
| [0004](0004-everything-is-a-plugin.md) | Kernel plus plugins: everything is a plugin | Accepted, partly superseded by [0024](0024-native-code-plugins.md) |
| [0005](0005-raw-input-and-layered-keymaps.md) | Raw input and layered key maps in the kernel | Accepted |
| [0006](0006-one-text-config-format.md) | One human-editable text format for configuration and data | Accepted |
| [0007](0007-content-based-locators.md) | Content-based reading positions | Accepted |
| [0008](0008-fonts-outlines-and-bitmap-packs.md) | Fonts: runtime outlines and bitmap packs | Accepted; the UI font is now Source Sans 3; kerning and ligatures came ([0029](0029-kerning-and-ligatures.md)) |
| [0009](0009-documents-formats-and-storage.md) | Documents, formats and local storage | Accepted |
| [0010](0010-overlays-menus-and-settings-ui.md) | Overlays, menus and the generated settings UI | Accepted |
| [0011](0011-epub-streaming-conversion.md) | EPUB: streaming conversion into a block cache | Accepted |
| [0012](0012-banded-rendering.md) | Rendering in bands | Accepted |
| [0013](0013-x4-firmware.md) | The Xteink X4's firmware | Accepted |
| [0014](0014-sockets-for-plugins.md) | Sockets for plugins | Accepted |
| [0015](0015-colour-pictures.md) | Colour pictures | Accepted |
| [0016](0016-network-and-wifi.md) | Network and Wi-Fi | Accepted |
| [0017](0017-tls-on-the-x4.md) | TLS on the X4 | Accepted |
| [0018](0018-font-packs-for-other-scripts.md) | Font packs for other scripts | Accepted |
| [0019](0019-firmware-updates.md) | Firmware updates | Accepted |
| [0020](0020-sdk-compatibility.md) | SDK compatibility | Accepted; plugins no longer name an SDK in their manifest ([0026](0026-plugin-interface-version.md)) |
| [0021](0021-decorations.md) | Decorations of text | Accepted |
| [0022](0022-plugins-in-updates.md) | Plugins in updates | Accepted |
| [0023](0023-plugin-store.md) | The plugin store | Accepted |
| [0024](0024-native-code-plugins.md) | Native code plugins as files | Accepted, partly implemented; supersedes part of [0004](0004-everything-is-a-plugin.md) |
| [0025](0025-power-saving.md) | Power saving: efficient by default, declared work, power modes | Accepted, partly implemented |
| [0026](0026-plugin-interface-version.md) | Code plugins bound to a plugin interface version, built without build scripts | Accepted |
| [0027](0027-bluetooth.md) | Bluetooth Low Energy | Accepted |
| [0029](0029-kerning-and-ligatures.md) | Kerning and ligatures | Accepted |
| [0030](0030-real-time-screens.md) | Screens that run in real time | Accepted |
| [0031](0031-developer-link.md) | A developer link over USB for plugins | Accepted; reserved, carried out later |
| [0032](0032-writing-between-the-lines.md) | Writing between a book's lines | Accepted; annotations reserved, carried out later |
| [0033](0033-exfat-cards.md) | exFAT cards | Accepted |

## Template

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD

## Context
What forces are at play?

## Decision
What we do.

## Consequences
What becomes easier, what becomes harder.
```
