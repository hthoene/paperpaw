use super::*;

#[test]
fn focus_wraps_round_the_controls() {
    assert_eq!(step_focus(0, -1, 3), 2);
    assert_eq!(step_focus(2, 1, 3), 0);
    assert_eq!(step_focus(1, 1, 3), 2);
    assert_eq!(step_focus(0, 1, 0), 0);
}
