//! Allocation-free REG declaration checks before catalog admission.
use super::{
    declarations::{full_id, need_value_check, relative},
    registrations::SetupStep,
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

/// Local setup syntax only; catalog references and name collisions are checked
/// when construction commits. Unknown device needs remain valid but unmet.
pub(crate) fn setup_step(plugin: &str, step: &SetupStep<'_>) -> Result<(), i32> {
    let SetupStep {
        name,
        title,
        help: _,
        order: _,
        page_kind,
        page,
        applies_key,
        applies_value,
        need,
        capability,
        flags,
    } = step;
    relative(plugin, name)?;
    if title.is_empty() || flags & !3 != 0 {
        return Err(ERR_INVALID);
    }
    if *page_kind > 1 {
        return Err(ERR_UNSUPPORTED);
    }
    full_id(page)?;
    if !page.contains('.') || applies_key.is_empty() != applies_value.is_empty() {
        return Err(ERR_INVALID);
    }
    if !applies_key.is_empty() {
        full_id(applies_key)?;
        if !applies_key.contains('.') {
            return Err(ERR_INVALID);
        }
    }
    if flags & 2 == 0 {
        if *need != 0 || *capability != 0 {
            return Err(ERR_INVALID);
        }
        Ok(())
    } else {
        need_value_check(*need, *capability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::{declarations::validate_record, registrations::Record};

    fn step() -> SetupStep<'static> {
        SetupStep {
            name: "first-run",
            title: "First run",
            help: "",
            order: 0,
            page_kind: 0,
            page: "fixture.configure",
            applies_key: "",
            applies_value: "",
            need: 0,
            capability: 0,
            flags: 1,
        }
    }

    #[test]
    fn setup_checks_device_need_presence_without_refusing_future_needs() {
        let base = step();
        assert_eq!(setup_step("fixture", &base), Ok(()));
        for row in [
            SetupStep { need: 1, ..base },
            SetupStep {
                capability: 1,
                ..base
            },
            SetupStep {
                flags: 3,
                need: 12,
                ..base
            },
            SetupStep {
                flags: 3,
                need: 1,
                capability: 1,
                ..base
            },
        ] {
            assert_eq!(setup_step("fixture", &row), Err(ERR_INVALID));
        }
        for row in [
            SetupStep {
                flags: 3,
                need: 1,
                ..base
            },
            SetupStep {
                flags: 3,
                need: 12,
                capability: 1,
                ..base
            },
            SetupStep {
                flags: 3,
                need: u32::MAX,
                capability: 1,
                ..base
            },
        ] {
            assert_eq!(setup_step("fixture", &row), Ok(()));
        }
    }

    #[test]
    fn setup_refuses_bad_names_flags_pages_and_applicability_before_admission() {
        let base = step();
        for row in [
            SetupStep { name: "", ..base },
            SetupStep { title: "", ..base },
            SetupStep { flags: 4, ..base },
            SetupStep {
                page: "relative",
                ..base
            },
            SetupStep {
                applies_key: "fixture.enabled",
                ..base
            },
            SetupStep {
                applies_value: "true",
                ..base
            },
            SetupStep {
                applies_key: "bad id",
                applies_value: "true",
                ..base
            },
        ] {
            assert_eq!(
                validate_record("fixture", &Record::SetupStep(row)),
                Err(ERR_INVALID)
            );
        }
        assert_eq!(
            validate_record("fixture", &Record::SetupStep(base)),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(
            setup_step(
                "fixture",
                &SetupStep {
                    page_kind: 2,
                    ..base
                }
            ),
            Err(ERR_UNSUPPORTED)
        );
        assert_eq!(
            setup_step(
                "fixture",
                &SetupStep {
                    page_kind: 1,
                    applies_key: "fixture.enabled",
                    applies_value: "true",
                    ..base
                }
            ),
            Ok(())
        );
    }
}
