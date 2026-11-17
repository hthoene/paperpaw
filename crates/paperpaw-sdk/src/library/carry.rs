//! [`try_moved`] and [`try_removed`]: what the library knows of a book
//! carried along or forgotten, with every card error the caller can
//! retry returned instead of dropped, for a host step that must fail and
//! run again (a power cut, a full card). An error no retry changes (a
//! path the card refuses, damaged data) does not stop the other parts:
//! they are still carried, and that error is returned at the end.













#[cfg(test)]
#[path = "carry_tests.rs"]
mod tests;
