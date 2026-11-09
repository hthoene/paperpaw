//! Op 15 `setup` (`registry.md` 2.2 and 6): the first-run setup steps in order,
//! `flags` bit 0 where the step applies now.
//!
//! Two sources, merged by `order` (the environment's first where equal): the
//! steps code plugins added (`SetupSteps`, whose `applying` evaluates their
//! `AppliesFn` here, in the host's process) and the `setup.step` declarations
//! of native plugins in the declaration store, whose applicability is
//! evaluated from the declaration (`registry.md` section 6, rules 1 to 3).
use super::{
    Row,
    fixed::{Fixed, caps, translated},
    ordered::{self, Sort},
    rules,
    sources::Sources,
};
use crate::native_host::{
    declaration_store::{DeclarationStore, Kind},
    registrations::{self as wire, Record},
};
use core::ops::ControlFlow;
use paperpaw_native::host_glue::ERR_BUSY;
use paperpaw_sdk::{Env, setup::SetupPage};

/// `reg_setup_step.flags`.
const APPLIES: u32 = 1;
/// `setup.step.flags`: the declaration's default applicability and whether
/// the step has a device requirement.
const DECLARED_APPLIES: u32 = 1;
const HAS_NEED: u32 = 1 << 1;

fn key_of(record: &Record<'_>) -> Option<i64> {
    match record {
        Record::SetupStep(step) => Some(i64::from(step.order)),
        _ => None,
    }
}

/// Whether a declared step applies now: its setting condition holds, the
/// device meets its need, and the plugin's last push (none is kept yet, so
/// the declaration's own bit) says so.
fn applies(env: &Env<'_>, step: &wire::SetupStep<'_>) -> bool {
    rules::value_holds(env.settings(), step.applies_key, step.applies_value)
        && (step.flags & HAS_NEED == 0 || rules::need_met(step.need, step.capability, env.caps()))
        && step.flags & DECLARED_APPLIES != 0
}

/// The row of a declared step.
fn declared_row(
    env: &Env<'_>,
    plugin: &str,
    step: &wire::SetupStep<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let Some(id) = Fixed::<{ caps::ID }>::of(&[plugin, ".", step.name]) else {
        return ControlFlow::Continue(());
    };
    visit(&Record::ServiceRegSetupStep(wire::ServiceRegSetupStep {
        id: id.as_str(),
        page_kind: step.page_kind,
        page: step.page,
        order: step.order,
        flags: if applies(env, step) { APPLIES } else { 0 },
        title: translated(env.i18n(), step.title, caps::TITLE),
    }))
}

pub(super) fn rows(
    sources: &Sources<'_>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> Result<(), i32> {
    let env = sources.env;
    let guard = match sources.directory {
        Some(directory) => Some(directory.store.try_borrow().map_err(|_| ERR_BUSY)?),
        None => None,
    };
    let _end = merged(env, guard.as_deref(), visit);
    Ok(())
}

fn merged(
    env: &Env<'_>,
    store: Option<&DeclarationStore>,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let steps = env.setup().applying(env);
    let mut own = steps.iter();
    let mut ahead = own.next();
    let mut after: Option<Sort> = None;
    let mut declared =
        store.and_then(|store| ordered::next_after(store, &[Kind::SetupStep], &key_of, after));
    loop {
        // The environment's step goes first unless the declared one is lower.
        let env_first = match (ahead, &declared) {
            (None, None) => return ControlFlow::Continue(()),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(step), Some(((order, _), _))) => i64::from(step.order) <= *order,
        };
        if env_first {
            if let Some(step) = ahead {
                env_row(env, step, visit)?;
            }
            ahead = own.next();
            continue;
        }
        let (Some(store), Some((sort, key))) = (store, declared) else {
            return ControlFlow::Continue(());
        };
        after = Some(sort);
        ordered::with_plugin(store, key, &mut |record, plugin| match record {
            Record::SetupStep(step) => declared_row(env, plugin, &step, visit),
            _ => ControlFlow::Continue(()),
        })?;
        declared = ordered::next_after(store, &[Kind::SetupStep], &key_of, after);
    }
}

/// The row of a step a code plugin added, which applies (`applying` kept only those).
fn env_row(
    env: &Env<'_>,
    step: &paperpaw_sdk::setup::SetupStepSpec,
    visit: &mut dyn FnMut(&dyn Row) -> ControlFlow<()>,
) -> ControlFlow<()> {
    let (page_kind, page) = match &step.page {
        SetupPage::Setting(id) => (0, id.as_str()),
        SetupPage::Screen(id) => (1, id.as_str()),
        _ => return ControlFlow::Continue(()),
    };
    // An empty title is the setting's own.
    let key: &str = if step.title.is_empty() {
        env.settings()
            .spec(page)
            .map_or("", |spec| spec.title.as_ref())
    } else {
        step.title.as_ref()
    };
    visit(&Record::ServiceRegSetupStep(wire::ServiceRegSetupStep {
        id: step.id.as_str(),
        page_kind,
        page,
        order: step.order,
        flags: APPLIES,
        title: translated(env.i18n(), key, caps::TITLE),
    }))
}
