### Changed
- A book whose format plugin sleeps opens as "preparing" and the format is woken to open it, instead of the reader being told "busy"; a format that cannot be woken, or finds no memory after its longest wait, fails the book with the usual error notice (#248).
