//! The plugin store's rules, as paperpaw.org checks them: one file,
//! `store-rules.json`, which the site serves as it is
//! (`/api/v1/store-rules.json`) and this crate holds a copy of. A test of
//! the site's keeps its constants and its file the same; PaperPaw's CI
//! compares this copy with the site's.

/// The rules of PaperPaw's own copy.
pub const BUILT_IN: &str = include_str!("../store-rules.json");

/// The format of the rules this crate reads.
const FORMAT: f64 = 1.0;

/// The store's rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rules {
    /// The groups a plugin belongs to, in the site's order.
    pub groups: Vec<String>,
    /// Ids of PaperPaw's own plugins taken out of the store: never used.
    pub retired: Vec<String>,
    pub max_id: usize,
    pub max_name: usize,
    pub max_summary: usize,
    pub max_description: usize,
    pub max_licence: usize,
    pub max_tags: usize,
    pub max_tag: usize,
    pub max_screenshots: usize,
    pub max_screenshot_bytes: u64,
    pub screenshot_types: Vec<String>,
    pub max_alt: usize,
    /// The kinds of files a plugin's folder may hold, by extension.
    pub package_kinds: Vec<String>,
    pub max_files: usize,
    pub max_path: usize,
    /// The folder's ZIP, and all its files unpacked.
    pub max_package: u64,
    pub max_content: u64,
    pub max_build: u64,
    pub max_source: u64,
    /// Files of a release the site reads at most.
    pub max_assets: usize,
    /// The shortest *How it works* of a plugin with code, in characters.
    pub least_how: usize,
    /// The readers a plugin with code has builds for, by their names.
    pub readers: Vec<String>,
    /// The GitHub topic that lists a repository.
    pub topic: String,
}

impl Rules {
    /// PaperPaw's own copy of the rules.
    pub fn built_in() -> Rules {
        match Rules::parse(BUILT_IN) {
            Ok(rules) => rules,
            // A test reads it; a release never has a broken copy.
            Err(e) => panic!("store-rules.json: {e}"),
        }
    }

    /// The rules in the JSON `text`, as the site serves them.
    pub fn parse(text: &str) -> Result<Rules, String> {
        let members = paperpaw_json::object(text);
        if members.is_empty() {
            return Err("not a JSON object".into());
        }
        let number = |key: &str| -> Result<u64, String> {
            paperpaw_json::number(&members, key)
                .filter(|n| *n >= 0.0 && n.fract() == 0.0)
                .map(|n| n as u64)
                .ok_or_else(|| format!("no number {key}"))
        };
        let list = |key: &str| -> Result<Vec<String>, String> {
            let raw =
                paperpaw_json::raw_member(text, key).ok_or_else(|| format!("no list {key}"))?;
            Ok(paperpaw_json::string_array(raw))
        };
        let format = paperpaw_json::number(&members, "format").unwrap_or(0.0);
        if format != FORMAT {
            return Err(format!(
                "rules of format {format}, and this tool reads {FORMAT}: update it"
            ));
        }
        let size = |key: &str| number(key).map(|n| n as usize);
        Ok(Rules {
            groups: list("groups")?,
            retired: list("retired")?,
            max_id: size("max_id")?,
            max_name: size("max_name")?,
            max_summary: size("max_summary")?,
            max_description: size("max_description")?,
            max_licence: size("max_licence")?,
            max_tags: size("max_tags")?,
            max_tag: size("max_tag")?,
            max_screenshots: size("max_screenshots")?,
            max_screenshot_bytes: number("max_screenshot_bytes")?,
            screenshot_types: list("screenshot_types")?,
            max_alt: size("max_alt")?,
            package_kinds: list("package_kinds")?,
            max_files: size("max_files")?,
            max_path: size("max_path")?,
            max_package: number("max_package")?,
            max_content: number("max_content")?,
            max_build: number("max_build")?,
            max_source: number("max_source")?,
            max_assets: size("max_assets")?,
            least_how: size("least_how")?,
            readers: list("readers")?,
            topic: paperpaw_json::text(&members, "topic")
                .ok_or("no text topic")?
                .to_string(),
        })
    }

    /// Whether `id` is a plugin's id by the site's rules: lower-case
    /// letters, digits and hyphens, not at its ends, at most `max_id`.
    pub fn valid_id(&self, id: &str) -> bool {
        valid_name(id, self.max_id)
    }

    /// Whether `tag` is a tag: as an id, at most `max_tag`.
    pub fn valid_tag(&self, tag: &str) -> bool {
        valid_name(tag, self.max_tag)
    }
}

/// Lower-case letters, digits and hyphens, not at its ends, at most `max`.
pub fn valid_name(name: &str, max: usize) -> bool {
    !name.is_empty()
        && name.len() <= max
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// Whether `text` is a version as the site takes one: SemVer, such as
/// `1.0.0` or `1.0.0-beta.1`, at most 40 characters.
pub fn is_version(text: &str) -> bool {
    let (numbers, pre) = text
        .split_once('-')
        .map_or((text, None), |(n, p)| (n, Some(p)));
    let parts: Vec<&str> = numbers.split('.').collect();
    let number = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && !(part.len() > 1 && part.starts_with('0'))
            && part.parse::<u32>().is_ok()
    };
    text.len() <= 40
        && parts.len() == 3
        && parts.iter().all(|p| number(p))
        && pre.is_none_or(|pre| {
            pre.split('.').all(|part| {
                let digits = part.bytes().all(|b| b.is_ascii_digit());
                !part.is_empty()
                    && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    && !(digits && part.len() > 1 && part.starts_with('0'))
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_reads_and_names_the_thirteen_groups() {
        let rules = Rules::built_in();
        assert_eq!(rules.groups.len(), 13);
        assert_eq!(rules.groups.last().map(String::as_str), Some("system"));
        assert_eq!(rules.max_tags, 5);
        assert_eq!(rules.readers, ["x4", "x4-pro", "x4-classic", "x3"]);
        assert!(
            Rules::parse("{\"format\": 2}")
                .unwrap_err()
                .contains("update")
        );
    }

    #[test]
    fn ids_and_versions() {
        let rules = Rules::built_in();
        assert!(rules.valid_id("night-owl2"));
        assert!(!rules.valid_id("Night"));
        assert!(!rules.valid_id("-x"));
        assert!(!rules.valid_id(&"a".repeat(41)));
        assert!(is_version("1.0.0"));
        assert!(is_version("1.0.0-beta.1"));
        assert!(!is_version("1.0"));
        assert!(!is_version("v1.0.0"));
        assert!(!is_version("01.0.0"));
    }
}
