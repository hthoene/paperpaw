//! What the converter does where an element starts.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, BlockKind, DocumentError};
use paperpaw_sdk::lang::{Direction, Lang};
use paperpaw_sdk::xml::{Attrs, local_name, try_decode};

use super::elements::{self, element_props, furniture, heading_level, is_block, styled};
use super::{
    Converter, Dialect, FLUSH_SMALL, Frame, MAX_CHILD_NAMES, MAX_DEPTH, MAX_HREF, MAX_PATH,
    direction, element_name, foreign, larger, lists, tables,
};
use crate::css::{self, Props};
use crate::fallible::{self, TryPush, TryPushStr};

/// What an element takes from the elements around it, or sets itself.
struct Inherited {
    indent: Option<i16>,
    size: Option<u16>,
    dir: Direction,
    lang: Lang,
    /// The controls that set its text apart, if it is inline text with a
    /// direction of its own.
    controls: Option<(char, char)>,
    margins: (u16, u16),
    line_height: Option<u16>,
}

impl Converter<'_> {
    pub(super) fn start(&mut self, name: &str, attrs: Attrs<'_>) -> Result<(), DocumentError> {
        if self.full {
            return Ok(());
        }
        if let Some(math) = self.math.as_mut() {
            return math.start(local_name(name), &attrs);
        }
        self.end_node();
        let tag = self.entered(element_name(name)?, &attrs)?;
        let props = if self.hidden == 0 {
            element_props(&tag, &attrs, &mut self.rules, self.styles)?
        } else {
            Props::default()
        };
        let mut hides = matches!(tag.as_str(), "head" | "script" | "style" | "title" | "svg")
            || props.hidden == Some(true);
        if !hides && let Some(html) = &self.html {
            hides = furniture(&tag, &attrs, html.focus.is_some())?;
        }
        self.end_before(&tag, &attrs, hides)?;
        // Self-closing elements are ended right away by the caller.
        let (style, align) = self.frame();
        let inherited = self.inherited(&tag, &attrs, &props)?;
        if self.stack.len() >= MAX_DEPTH {
            // Read as part of the deepest element kept.
            self.overflow += 1;
            return Ok(());
        }
        let index = self.child_index(&tag)?;
        let link = self.link(&tag, &attrs, hides)?;
        let path = self.enter_path(&tag, index)?;
        let focus = self
            .html
            .as_ref()
            .and_then(|html| html.focus.as_deref())
            .is_some_and(|focus| focus == tag && !self.stack.iter().any(|f| f.focus));
        if focus {
            // The reading matter begins. It is shown, unless hidden otherwise.
            self.hidden = self.hidden.saturating_sub(1);
        }
        let frame_tag = fallible::string(&tag)?;
        let quote = tables::Quoted::of(
            &tag,
            &attrs,
            self.stack.last().map_or_else(Default::default, |f| f.quote),
        )?;
        let Inherited {
            indent,
            size,
            dir,
            lang,
            controls,
            margins,
            line_height,
        } = inherited;
        self.stack.try_push(Frame {
            quote,
            link,
            focus,
            children: Vec::new(),
            path,
            nodes: 0,
            texts: 0,
            block: is_block(&tag)
                || matches!(tag.as_str(), "li" | "pre" | "td" | "th" | "thead" | "tbody"),
            tag: frame_tag,
            style: styled(style, &props),
            align: props.align.unwrap_or(align),
            dir,
            lang,
            indent,
            size,
            margins,
            line_height,
            numbered: lists::counter(&tag, &attrs)?,
            list_style: lists::list_style(&tag, &props, self.stack.last()),
            after: (props.margin_bottom, props.padding_bottom),
            hides,
            closer: controls.map(|(_, close)| close),
        })?;
        let started = self.started(&tag, &attrs, hides, controls);
        if started.is_ok() && self.hidden == 0 && tag == "math" {
            self.math = Some(foreign::MAKER(&attrs)?);
        }
        started
    }

    /// The name `tag` is read as, and what the markup's dialect asks for
    /// where it starts.
    fn entered(&mut self, tag: String, attrs: &Attrs<'_>) -> Result<String, DocumentError> {
        let entered = match self.dialect.as_mut() {
            Some(dialect) => dialect.enter(&tag, attrs)?,
            None => return Ok(tag),
        };
        if let Some(id) = entered.anchor {
            self.flush()?;
            self.add_anchor(&id)?;
        }
        if entered.line_break && !self.text.is_empty() {
            self.push('\n')?;
        }
        Ok(entered.name)
    }

    /// Ends what element `tag` ends where it starts, and places its anchor.
    fn end_before(
        &mut self,
        tag: &str,
        attrs: &Attrs<'_>,
        hides: bool,
    ) -> Result<(), DocumentError> {
        // HTML leaves paragraphs and list items open: the next one ends
        // them. So do careless EPUBs with paragraphs, which XHTML never
        // nests.
        let open = self.stack.last().map(|f| f.tag.as_str());
        let closes = match (self.html.is_some(), tag) {
            _ if self.overflow > 0 => false,
            (false, "p") => self.dialect.is_none() && open == Some("p"),
            (false, _) => false,
            (true, "li") => matches!(open, Some("li" | "p")),
            (true, "dt" | "dd") => matches!(open, Some("dt" | "dd" | "p")),
            (true, "td" | "th") => matches!(open, Some("td" | "th")),
            (true, "tr") => matches!(open, Some("tr" | "td" | "th")),
            (true, tag) => is_block(tag) && open == Some("p"),
        };
        if closes && let Some(open) = open.map(fallible::string).transpose()? {
            self.close(&open)?;
        }
        // Block elements end the block before them, in the old style. This
        // comes before their anchor is placed, so it points at the new block.
        if !hides && self.hidden == 0 && (is_block(tag) || matches!(tag, "li" | "pre" | "hr")) {
            match self.in_row() {
                // In a cell: a line of it.
                true => self.line_in_cell()?,
                false => self.flush()?,
            }
        }
        if let Some(id) = attrs.try_get("id").map_err(|_| fallible::OOM)? {
            self.add_anchor(&id)?;
            if self.links.len() >= FLUSH_SMALL {
                self.write_links();
            }
        }
        Ok(())
    }

    /// What element `tag` inherits or sets itself. Where it starts a block,
    /// its space above, and the drop capital it asks for.
    fn inherited(
        &mut self,
        tag: &str,
        attrs: &Attrs<'_>,
        props: &Props,
    ) -> Result<Inherited, DocumentError> {
        if self.text.is_empty() {
            self.margin = larger(self.margin, props.margin_top);
            self.padding = css::sum(self.padding, props.padding_top);
        }
        let parent = self.stack.last();
        let indent = props.indent.or_else(|| parent.and_then(|f| f.indent));
        let parent_size = parent.and_then(|f| f.size);
        let size = match props.size {
            Some(size) => Some(size.of(parent_size.unwrap_or(100))),
            None => parent_size,
        };
        // A drop capital: an element that starts a paragraph asks for one.
        if self.text.is_empty() && !is_block(tag) && self.drop_cap.1.is_none() {
            let lines = elements::drop_cap(props, size, parent_size);
            self.drop_cap = (lines, (lines > 0).then_some(self.stack.len()));
        }
        // Paragraphs take the direction of the block elements around them.
        // Inline ones are not set apart yet. Text takes the language of the
        // innermost element that names one.
        let (parent_dir, parent_lang) =
            parent.map_or((self.book.dir, self.book.lang), |f| (f.dir, f.lang));
        let paragraphs = is_block(tag) || matches!(tag, "html" | "body" | "li" | "pre");
        let dir = direction::own_dir(attrs, props.direction)?
            .filter(|_| paragraphs)
            .unwrap_or(parent_dir);
        let lang = direction::own_lang(attrs)?.unwrap_or(parent_lang);
        let controls = direction::inline_controls(tag, attrs, paragraphs)?;
        // Blocks are set in by their own margins at the sides and by those
        // of the blocks around them. Text elements are not. The body's own
        // margins are the page's: those are the reader's margins, set in the
        // settings.
        let parent_margins = parent.map_or((0, 0), |f| f.margins);
        let margins = match is_block(tag) || matches!(tag, "li" | "pre") {
            true => direction::set_in(parent_margins, props.sides(), dir, tag == "body"),
            false => parent_margins,
        };
        let line_height = props
            .line_height
            .or_else(|| parent.and_then(|f| f.line_height));
        Ok(Inherited {
            indent,
            size,
            dir,
            lang,
            controls,
            margins,
            line_height,
        })
    }

    /// The number of element `tag` among its parent's children of the
    /// same name, counted in the parent.
    fn child_index(&mut self, tag: &str) -> Result<u32, DocumentError> {
        let hash = paperpaw_sdk::digest::fnv1a64(tag);
        let Some(parent) = self.stack.last_mut() else {
            return Ok(1);
        };
        parent.nodes += 1;
        Ok(match parent.children.iter_mut().find(|(t, _)| *t == hash) {
            Some((_, count)) => {
                *count += 1;
                *count
            }
            None => {
                if parent.children.len() < MAX_CHILD_NAMES {
                    parent.children.try_push((hash, 1))?;
                }
                1
            }
        })
    }

    /// Where element `tag` links to, if it is a link that is shown: where
    /// it starts, and its target.
    fn link(
        &self,
        tag: &str,
        attrs: &Attrs<'_>,
        hides: bool,
    ) -> Result<Option<(u32, String)>, DocumentError> {
        // `href`, also as FictionBook writes it (`l:href`). A link inside
        // a link is none, as in HTML, so one target at most is kept.
        let href = attrs
            .iter()
            .find(|(name, _)| local_name(name) == "href")
            .map(|(_, value)| try_decode(value))
            .transpose()
            .map_err(|_| fallible::OOM)?;
        Ok(match href {
            Some(href)
                if tag == "a"
                    && !hides
                    && self.hidden == 0
                    && !self.stack.iter().any(|f| f.link.is_some()) =>
            {
                self.target(&href)?
                    .filter(|target| target.len() <= MAX_HREF)
                    .map(|target| (self.here(), target))
            }
            _ => None,
        })
    }

    /// Adds element `tag`, number `index` of its name, to the path below
    /// `<body>`. The path's length before it.
    fn enter_path(&mut self, tag: &str, index: u32) -> Result<usize, DocumentError> {
        let path = self.path.len();
        if self.body.is_some() {
            let segment = fallible::try_format!("{tag}[{index}]")?;
            if path + 1 + segment.len() <= MAX_PATH {
                if path > 0 {
                    self.path.try_push('/')?;
                }
                self.path.try_push_str(&segment)?;
            }
        } else if tag == "body" {
            self.body = Some(self.stack.len());
        }
        Ok(path)
    }

    /// What element `tag` starts, now that its frame is on the stack.
    fn started(
        &mut self,
        tag: &str,
        attrs: &Attrs<'_>,
        hides: bool,
        controls: Option<(char, char)>,
    ) -> Result<(), DocumentError> {
        if hides {
            self.hidden += 1;
        }
        if let Some((open, _)) = controls {
            self.open_inline(open)?;
        }
        // Images, also inside <svg>, a common way to show covers.
        if matches!(tag, "img" | "image")
            && self.html.as_ref().is_none_or(|html| html.pictures)
            && self.only_svg_hides()
        {
            return self.start_image(attrs);
        }
        if self.hidden > 0 || self.enter_table(tag, attrs)? {
            return Ok(());
        }
        match tag {
            "br" => {
                if !self.text.is_empty() {
                    self.push('\n')?;
                }
                self.space = false;
            }
            "hr" if !self.in_row() => {
                self.align = Some(Align::Center);
                self.push_str("* * *")?;
                self.flush()?;
            }
            "li" => self.list_item(attrs)?,
            "pre" => {
                self.kind = BlockKind::Preformatted;
                self.pre += 1;
            }
            tag if is_block(tag) && !self.in_row() => {
                if let Some(level) = heading_level(tag) {
                    self.kind = BlockKind::Heading(level);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// The picture of an `<img>` or `<image>`, unless it has no address or
    /// its bytes are in the address itself.
    fn start_image(&mut self, attrs: &Attrs<'_>) -> Result<(), DocumentError> {
        let href = attrs
            .iter()
            .find(|(name, _)| matches!(local_name(name), "src" | "href"))
            .map(|(_, value)| try_decode(value))
            .transpose()
            .map_err(|_| fallible::OOM)?;
        if let Some(href) = href.filter(|h| !h.is_empty() && !h.starts_with("data:")) {
            let alt = attrs
                .try_get("alt")
                .map_err(|_| fallible::OOM)?
                .unwrap_or_default();
            self.image(&href, &alt)?;
        }
        Ok(())
    }
}
