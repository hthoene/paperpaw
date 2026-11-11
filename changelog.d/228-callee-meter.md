### Fixed

- **A plugin called by another plugin is metered as itself** (#228). While a native plugin answers a directed stage call (the document and lookup services), the usage meter charges the callee, and the caller's time stops until the call returns. A refused call charges nobody.
