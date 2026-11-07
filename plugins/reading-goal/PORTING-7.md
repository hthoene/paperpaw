# Interface-7 preparation

Base: checked PR #268 c4afa03b7. Screen identity/title encoding parity only.

Missing: widget/image schemas, READ_TODAY, foreign choices and DEV plugin lifecycle. Runtime behavior, instance policy, actions, menus and other declarations are not yet ported. No ready PR or runtime-size claim. Keep en/de and goldens unchanged. Patch-bump with runtime port.

Native store build migration must switch to paperpaw-plugin7.x and set `-C jump-tables=no`, then pass the 2 KiB frame scan. Current shared store template remains interface 6; todo 22 stays open.
