//! A native plugin named like the host is denied whole.
#![allow(clippy::unwrap_used, reason = "fixture errors must fail the test")]
use super::*;

#[test]
fn a_native_plugin_named_like_the_host_is_denied_whole() {
    reset(NORMAL);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let before = kernel.shared.registry.screens.list().iter().count();
    let result = kernel.register_native(
        host_named("core", 0),
        &FlatDepth::try_new().unwrap(),
        |_, _, _, _| {},
    );
    assert!(
        matches!(
            result,
            Err(Error::Status(host_glue::ERR_DENIED) | Error::Factory(host_glue::ERR_DENIED))
        ),
        "{result:?}"
    );
    assert_eq!(kernel.shared.registry.screens.list().iter().count(), before);
}
