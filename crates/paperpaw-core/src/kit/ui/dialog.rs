//! A dialog over a screen: a framed box with a title, a few lines of text
//! and a row of buttons, the same space all round and between its parts
//! (the style's margin). Where it goes on the screen is the caller's
//! ([`Dialog::place`]); what the buttons do too ([`Dialog::hit`], and the
//! focus moved with [`Dialog::step`]). Nothing in it is ever shortened:
//! where title and text do not fit on the screen even in smaller sizes,
//! they scroll above the buttons ([`Dialog::on_scroll_action`],
//! [`Dialog::on_gesture`]).
