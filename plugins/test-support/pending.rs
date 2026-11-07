//! Acceptance cases whose host path is not qualified yet. A case names its
//! interface-6 golden and the missing host pieces; it never passes by itself.
//!
//! Replace a case only with a run through the production loader that compares
//! the frame with the unchanged golden (shells/desktop/tests/support/NATIVE-UI.md).
use std::path::Path;

/// One acceptance case of a port.
pub struct Case {
    /// The interface-6 snapshot (`shells/desktop/tests/snapshots/<name>.png`).
    pub golden: &'static str,
    /// What is missing: H1 screen commands, H2 dialogs, H3 composition, H4 input.
    pub blocked_by: &'static [&'static str],
}

impl Case {
    /// The golden must still exist: the port is accepted against it.
    pub fn golden_exists(&self) -> bool {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../shells/desktop/tests/snapshots")
            .join(format!("{}.png", self.golden))
            .is_file()
    }

    /// The explicit refusal. Never turn this into success.
    pub fn refuse(&self) -> Result<(), String> {
        Err(format!(
            "BLOCKED {}: native acceptance of {} is not qualified",
            self.blocked_by.join("/"),
            self.golden
        ))
    }
}
