//! EPUB and Markdown as format plugins of the kernel under test: both are
//! native plugins only now (interface 6 is removed, #244) and the kernel does
//! not host those here, so the tests register the formats themselves.

#![allow(
    dead_code,
    reason = "shared by the tests; not every test file uses every format"
)]

/// EPUB as a format plugin of the kernel under test: EPUB is a native plugin
/// only now (interface 6 is removed, #244) and the kernel does not host
/// those here, so the tests register the format (`EpubDocument`) themselves.
pub struct TestEpub;

impl paperpaw_sdk::Plugin for TestEpub {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            paperpaw_epub_family::PLUGIN,
            "EPUB",
            paperpaw_sdk::package_version!(),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_book_cache(paperpaw_sdk::Env::plugin_dir("format-epub"), "")?;
        r.add_format(
            paperpaw_sdk::FormatSpec::new(
                paperpaw_sdk::Id::new_static("format-epub.epub"),
                "format-epub.title",
                vec![std::borrow::Cow::Borrowed("epub")],
                Box::new(|storage, path, options| {
                    paperpaw_epub_family::EpubDocument::open_options(storage, path, options)
                        .map(|d| Box::new(d) as Box<dyn paperpaw_sdk::document::Document>)
                }),
            )
            .prepared(Box::new(|storage, path, options| {
                paperpaw_epub_family::EpubDocument::prepared(storage, path, options)
            })),
        )?;
        r.add_translations(std::borrow::Cow::Borrowed(include_str!(
            "../../../format-epub/i18n/en.ini"
        )))?;
        Ok(())
    }
}

/// The built-in Markdown (`.md`) as a format plugin, as `TestEpub` is EPUB.
pub struct TestMarkdown;

impl paperpaw_sdk::Plugin for TestMarkdown {
    fn manifest(&self) -> paperpaw_sdk::Manifest {
        paperpaw_sdk::Manifest::new(
            paperpaw_sdk::Id::new_static("format-markdown"),
            "Markdown",
            paperpaw_sdk::package_version!(),
        )
        .uses(paperpaw_sdk::Uses::NOTHING)
    }

    fn register(&self, r: &mut paperpaw_sdk::Registrar) -> Result<(), paperpaw_sdk::RegisterError> {
        r.add_book_cache(paperpaw_sdk::Env::plugin_dir("format-epub"), "md-")?;
        r.add_format(paperpaw_sdk::FormatSpec::new(
            paperpaw_sdk::Id::new_static("format-markdown.markdown"),
            "format-markdown.title",
            vec![
                std::borrow::Cow::Borrowed("md"),
                std::borrow::Cow::Borrowed("markdown"),
            ],
            Box::new(|storage, path, options| {
                paperpaw_epub_family::MarkdownDocument::open(
                    storage,
                    path,
                    options.publisher_styles,
                )
                .map(|d| Box::new(d) as Box<dyn paperpaw_sdk::document::Document>)
            }),
        ))?;
        r.add_translations(std::borrow::Cow::Borrowed(include_str!(
            "../../../format-markdown/i18n/en.ini"
        )))?;
        Ok(())
    }
}
