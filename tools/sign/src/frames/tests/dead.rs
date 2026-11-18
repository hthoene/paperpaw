//! Code no path from the start reaches: read from the lowest `sp` the paths
//! from the start reach, not from the function's entry.

use super::*;

#[test]
fn a_dead_block_that_lowers_sp_is_counted_from_the_frame_the_function_has() {
    // A frame of 16, given back and returned; then a block no path reaches
    // that lowers `sp` by 2,032 (below the limit on its own, over it on top
    // of the frame the function has).
    let items = [
        W(addi(SP, SP, -16)),
        W(addi(SP, SP, 16)),
        W(RET),
        W(addi(SP, SP, -2032)),
        W(addi(SP, SP, 2032)),
        W(RET),
    ];
    assert!(matches!(
        found(&items),
        Some(Finding::Large { bytes: 2048, .. })
    ));
    // With no frame to add it to, the same block is under the limit.
    let items = [
        W(RET),
        W(addi(SP, SP, -2032)),
        W(addi(SP, SP, 2032)),
        W(RET),
    ];
    assert_eq!(found(&items), None);
}

#[test]
fn dead_blocks_do_not_add_up_each_starts_from_the_same_floor() {
    // Two dead blocks of 1,040 each, after a frame of 16: each is 1,056 from
    // the start, and neither is seeded from where the other went.
    let items = [
        W(addi(SP, SP, -16)),
        W(addi(SP, SP, 16)),
        W(RET),
        W(addi(SP, SP, -1040)),
        W(addi(SP, SP, 1040)),
        W(RET),
        W(addi(SP, SP, -1040)),
        W(addi(SP, SP, 1040)),
        W(RET),
    ];
    assert_eq!(found(&items), None);
}

#[test]
fn a_dead_block_that_gives_sp_back_does_not_hide_the_frame() {
    // The dead block raises `sp` above the start: the frame is the reachable
    // code's, as before.
    let items = [
        W(addi(SP, SP, -16)),
        W(addi(SP, SP, 16)),
        W(RET),
        W(addi(SP, SP, 64)),
        W(RET),
    ];
    assert_eq!(found(&items), None);
}
