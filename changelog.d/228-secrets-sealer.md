### Fixed

- **Wi-Fi and mail passwords keep working on a device that seals secrets** (#228). Where `core.secrets` seals values (the X4's device key), the kernel's interface-6 code now reads sealed lines and writes every save sealed, so a password kept by a native plugin and one kept by an interface-6 plugin share the file without losing each other's values. A save that cannot be sealed (on the X4 the radio must be on for a fresh nonce) is not written and not marked saved; nothing is ever written in the veiled form on such a device.

- A shell that wires `core.secrets` now tells the kernel (`Kernel::wire_secrets`), so interface 6's passwords are sealed even when no native plugin runs. A password with no owner in its id (no `.`) is refused at save instead of blocking later saves; an old such line is kept as it was.
