//! Another look for the parts every screen draws with this module: a
//! plugin gives switches, ticked boxes, the focus, separators, buttons,
//! headers, the row of button hints, progress bars and sliders a look
//! of its own, for all plugins at once.
//!
//! The SDK keeps where a part goes, what it says to screen readers and
//! how it is touched; a look only paints the part's rectangle. A painter
//! returns whether it painted (else the part is drawn as usual), and the
//! painters of parts with a label on them return the ink for it.
//!
//! The look in use is the one set with the highest priority (the later
//! of equal ones); [`UiStyle::new`](super::UiStyle::new) takes it from
//! [`Env::look`](crate::Env::look). A look is a table in the program,
//! so it takes no memory while it is not in use.
































