### Added

- `paperpaw-sign frames-s3` budgets take `allow form <function> <form> <why>` for the privileged forms firmware code uses (`rsil`, `waiti`, `wsr`, `xsr`, `wsr.ps`, `xsr.ps`, `l32e`, `s32e`). Each is named in the scan with what it writes, fails unless the budget allows it for that function, and one that writes `a1`, or a `wsr`/`xsr` of `WINDOWBASE` or `WINDOWSTART`, is still refused as a change of the stack pointer (only `allow sp` passes it). `PS` is its own form. `rotw`, the `rf*` returns, MAC16 and the `ee.*` forms stay refused.
