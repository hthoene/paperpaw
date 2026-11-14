//! The choices for the pictures of a book: shown, a short line in their place
//! (a caption of their alternative text), or left out. The reader applies the
//! choice to each picture block through the `reader.filter` stage.

/// Pictures stay as the book has them.
pub const SHOW: &str = "aids.pictures.show";
/// A line in place of each picture: `[Picture: alternative text]`.
pub const CAPTION: &str = "aids.pictures.caption";
/// Pictures are left out.
pub const HIDE: &str = "aids.pictures.hide";

/// The choices of the setting `aids.pictures`.
pub const CHOICES: [&str; 3] = [SHOW, CAPTION, HIDE];
