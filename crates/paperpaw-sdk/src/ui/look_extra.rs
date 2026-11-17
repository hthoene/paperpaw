//! What a look can paint besides the fixed parts of [`look`](super::look): the text of
//! the interface, and parts of kinds this release does not name yet.
//!
//! Both are built by the SDK code of the plugin that draws the screen and
//! read by the painter of the plugin that provides the look: two programs,
//! compiled at different times. So the types have private spare fields,
//! zero now and read through methods later, and the kinds are constants of
//! open types.
//!
//! A painter changes how a thing *looks*, never where it goes or how wide
//! it is: layout is measured with the stock metrics, and a look that
//! changed advances would break wrapping, hit areas and screen readers.
