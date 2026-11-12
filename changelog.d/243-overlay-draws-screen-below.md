### Fixed

- **A native screen under an overlay is drawn** (#243). The page below the reader's menu bar, a panel,
  a lookup or a dialog drew blank: the host drew a native screen only when it was on top. As
  `ui.md` §1.2 says, a screen with only overlays above it is drawn as if on top, the overlays after it;
  input still reaches the top screen only. Under another plugin's overlay a screen that is secret now
  (a run-time policy too) or on a stack with a guard (a lock's PIN dialog included) stays blank.
