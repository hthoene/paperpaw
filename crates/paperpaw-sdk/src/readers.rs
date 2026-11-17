//! Opening books, and the reading-app choices kept for books and folders.
//!
//! - Whatever opens a book (the library, the file manager, the browser, a
//!   catalog) opens it with [`open`] ([`Command::OpenBook`]): the kernel
//!   opens the screen bound to [`slots::READER`](crate::builtin::slots::READER) with what [`opened`]
//!   passes on. A reading app that cannot open a book after all says so
//!   with `cannot_open`.
//! - The choices for a book and for a folder are kept ([`remember`]): the
//!   book's moved and removed with it, the folders' in one small file
//!   (moved with [`folder_moved`] by whatever moves a folder). [`ask`] lets
//!   the reader choose (*Open with*, a folder's reading app).
