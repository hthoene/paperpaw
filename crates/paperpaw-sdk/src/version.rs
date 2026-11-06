use core::fmt;

/// A semantic version (`major.minor.patch`, with a pre-release such as
/// `0.5.0-beta.2`), ordered as SemVer orders versions: a pre-release
/// before its release, `alpha` before `beta` before `dev` before `pre`
/// before `rc`, a number after the name below them (`beta` before
/// `beta.2` before `beta.10`). Build metadata (`+abc`) is not kept: it
/// orders nothing.
///
/// A pre-release is one of `alpha`, `beta`, `dev`, `pre` and `rc`, with
/// an optional number up to `MAX_PRE_NUMBER` (`rc.3`): it takes 2
/// bytes, so a version is 8 bytes, as the device has many of them. Any
/// other pre-release is not a version here ([`Version::try_parse`] says
/// why).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    /// 0xFFFF for a release, else the pre-release's code (see `pre_code`):
    /// smaller than a release, in SemVer's order.
    pre: u16,
}

/// The pre-release numbers there is room for: `beta.0` to `beta.8190`.
pub const MAX_PRE_NUMBER: u16 = 8190;

/// The pre-release names, in SemVer's order: by their letters.
const PRE_NAMES: [&str; 5] = ["alpha", "beta", "dev", "pre", "rc"];

/// A release's `pre`.
const RELEASE: u16 = u16::MAX;

/// Why a text is no [`Version`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VersionError {
    /// Not `major.minor.patch` (or `major.minor`, or `major`) of numbers.
    Numbers,
    /// A pre-release that is none of `alpha`, `beta`, `dev`, `pre` and
    /// `rc` with an optional number up to [`MAX_PRE_NUMBER`].
    PreRelease,
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            VersionError::Numbers => "a version is written like 1.2.3",
            VersionError::PreRelease => {
                "a pre-release is alpha, beta, dev, pre or rc, with an optional \
                 number up to 8190 after a dot (beta.2), as in 1.2.3-beta.2"
            }
        })
    }
}

/// The SDK version implemented by this crate: this release's, a
/// pre-release of it between releases.
pub const SDK_VERSION: Version = crate::package_version!();

/// The version of the formats of data plugins this PaperPaw reads (their
/// `plugin.ini`, translations, key maps, `settings.ini`, styles; ADR
/// 0020). A data plugin says which it is made for with `formats` in
/// `[plugin]` (1 if it does not); one made for a later version is not
/// loaded, rather than read wrongly. 2 adds `marks.ini` (see
/// [`decorations::MarkRules`](crate::decorations::MarkRules)); 3 adds
/// `menus.ini`, `actions.ini`, `icons.ini`, `points/`, `setup.ini` and
/// `[fonts]` in `plugin.ini`; 4 adds compiled catalogs, `i18n/*.ppcat`
/// (read as needed, ADR 0033); 5 adds `[hyphenation]` in `plugin.ini`
/// and its tables, `hyphenation/*.pphp`.
pub const DATA_FORMATS: u32 = 5;

/// `a` is `b`, as bytes (const).
const fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The code of the pre-release `text` (`beta.2`), if it is one: the name's
/// place in `PRE_NAMES` from 1 in the top bits, below it 0 for no number,
/// else the number and 1.
const fn pre_code(text: &[u8]) -> Option<u16> {
    let mut dot = text.len();
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'.' {
            dot = i;
            break;
        }
        i += 1;
    }
    let (name, rest) = text.split_at(dot);
    let mut kind = 0;
    let mut n = 0;
    while n < PRE_NAMES.len() {
        if same(name, PRE_NAMES[n].as_bytes()) {
            kind = n as u16 + 1;
        }
        n += 1;
    }
    if kind == 0 {
        return None;
    }
    let number = if rest.is_empty() {
        0
    } else {
        // A dot, then digits only, no leading zero, at most 8190.
        let digits = rest.split_at(1).1;
        if digits.is_empty() || digits.len() > 4 || (digits.len() > 1 && digits[0] == b'0') {
            return None;
        }
        let mut value: u16 = 0;
        let mut d = 0;
        while d < digits.len() {
            if !digits[d].is_ascii_digit() {
                return None;
            }
            value = value * 10 + (digits[d] - b'0') as u16;
            d += 1;
        }
        if value > MAX_PRE_NUMBER {
            return None;
        }
        value + 1
    };
    Some(kind << 13 | number)
}

impl Version {
    /// A release: `major.minor.patch`, no pre-release.
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
            pre: RELEASE,
        }
    }

    /// The same numbers with the pre-release `pre` (`beta.2`); `None`
    /// where it is none of those a version holds (see [`Version`]).
    pub const fn with_pre(self, pre: &str) -> Option<Self> {
        match pre_code(pre.as_bytes()) {
            Some(code) => Some(Self { pre: code, ..self }),
            None => None,
        }
    }

    /// Whether it is a pre-release of the version its numbers name.
    pub const fn is_pre_release(&self) -> bool {
        self.pre != RELEASE
    }

    /// The pre-release's name (`beta`, of `0.5.0-beta.2`), if it is one.
    pub fn pre_name(&self) -> Option<&'static str> {
        self.is_pre_release()
            .then(|| PRE_NAMES[usize::from(self.pre >> 13) - 1])
    }

    /// The pre-release's number (`2`, of `0.5.0-beta.2`), if it has one.
    pub fn pre_number(&self) -> Option<u16> {
        let number = self.pre & 0x1FFF;
        (self.is_pre_release() && number > 0).then(|| number - 1)
    }

    /// The release it is of: the same numbers, no pre-release.
    pub const fn release(self) -> Self {
        Self::new(self.major, self.minor, self.patch)
    }

    /// The digits of `text` as a number, at compile time (for
    /// [`package_version!`](crate::package_version)); 0 where it has none.
    #[doc(hidden)]
    pub const fn number(text: &str) -> u16 {
        let bytes = text.as_bytes();
        let mut value: u16 = 0;
        let mut i = 0;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            value = value
                .saturating_mul(10)
                .saturating_add((bytes[i] - b'0') as u16);
            i += 1;
        }
        value
    }

    /// A crate's version from its `Cargo.toml` (for
    /// [`package_version!`](crate::package_version)); a pre-release a
    /// version cannot hold stops the build, saying so:
    ///
    /// ```
    /// const V: paperpaw_sdk::Version = paperpaw_sdk::Version::of_package("1", "0", "0", "beta.2");
    /// assert_eq!(V.pre_name(), Some("beta"));
    /// ```
    ///
    /// ```compile_fail
    /// const V: paperpaw_sdk::Version = paperpaw_sdk::Version::of_package("1", "0", "0", "gamma");
    /// ```
    #[doc(hidden)]
    pub const fn of_package(major: &str, minor: &str, patch: &str, pre: &str) -> Self {
        let version = Self::new(
            Self::number(major),
            Self::number(minor),
            Self::number(patch),
        );
        if pre.is_empty() {
            return version;
        }
        match version.with_pre(pre) {
            Some(version) => version,
            None => panic!(
                "this crate's version has a pre-release PaperPaw does not take: use alpha, beta, dev, pre or rc, with an optional number up to 8190 after a dot (1.2.3-beta.2)"
            ),
        }
    }

    /// Parses `1`, `1.2` or `1.2.3`, and after three numbers a pre-release
    /// (`1.2.3-beta.1`) and build metadata (`1.2.3+abc`, read and left
    /// out); `None` for anything else.
    pub fn parse(text: &str) -> Option<Version> {
        Self::try_parse(text).ok()
    }

    /// [`Version::parse`], saying what is wrong where it is none.
    pub fn try_parse(text: &str) -> Result<Version, VersionError> {
        let text = text.trim();
        let (rest, build) = text
            .split_once('+')
            .map_or((text, None), |(r, b)| (r, Some(b)));
        let (numbers, pre) = rest
            .split_once('-')
            .map_or((rest, None), |(n, p)| (n, Some(p)));
        if (pre.is_some() || build.is_some()) && numbers.split('.').count() != 3 {
            return Err(VersionError::Numbers);
        }
        // Build metadata: parts of letters, digits and `-`.
        if build.is_some_and(|build| {
            build.split('.').any(|part| {
                part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        }) {
            return Err(VersionError::Numbers);
        }
        let mut parts = numbers.split('.');
        let mut number = |optional: bool| -> Result<u16, VersionError> {
            match parts.next() {
                None if optional => Ok(0),
                Some(part) if !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()) => {
                    part.parse().map_err(|_| VersionError::Numbers)
                }
                _ => Err(VersionError::Numbers),
            }
        };
        let (major, minor, patch) = (number(false)?, number(true)?, number(true)?);
        if parts.next().is_some() {
            return Err(VersionError::Numbers);
        }
        let version = Version::new(major, minor, patch);
        match pre {
            Some(pre) => version.with_pre(pre).ok_or(VersionError::PreRelease),
            None => Ok(version),
        }
    }

    /// Whether code requiring `required` works with this version, following
    /// SemVer (for `0.x`, the minor version must match). By their numbers:
    /// a pre-release of a version has that version's interface.
    pub fn satisfies(&self, required: &Version) -> bool {
        let same_line = if required.major == 0 {
            self.major == 0 && self.minor == required.minor
        } else {
            self.major == required.major
        };
        same_line && self.release() >= required.release()
    }
}

/// The version of the crate it is written in (its `Cargo.toml`'s,
/// pre-release included), for a plugin's
/// [`Manifest::version`](crate::Manifest): a plugin released with its crate
/// says the same version everywhere. A pre-release other than `alpha`,
/// `beta`, `dev`, `pre` and `rc` with an optional number (`-beta.2`) stops
/// the build with a message that says so.
///
/// ```
/// const VERSION: paperpaw_sdk::Version = paperpaw_sdk::package_version!();
/// assert_eq!(format!("{VERSION}"), env!("CARGO_PKG_VERSION"));
/// ```
#[macro_export]
macro_rules! package_version {
    () => {
        $crate::Version::of_package(
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH"),
            env!("CARGO_PKG_VERSION_PRE"),
        )
    };
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(name) = self.pre_name() {
            write!(f, "-{name}")?;
            if let Some(number) = self.pre_number() {
                write!(f, ".{number}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Version({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::string::ToString;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn a_version_is_eight_bytes() {
        assert_eq!(core::mem::size_of::<Version>(), 8);
    }

    #[test]
    fn sdk_version_matches_crate_version() {
        // The crate's version, a pre-release between releases.
        assert_eq!(Version::parse(env!("CARGO_PKG_VERSION")), Some(SDK_VERSION));
    }

    #[test]
    fn pre_releases_are_kept_and_builds_left_out() {
        let beta = |n: &str| Version::new(0, 5, 0).with_pre(n).unwrap();
        assert_eq!(Version::parse("0.5.0-beta.2"), Some(beta("beta.2")));
        assert_eq!(beta("beta.2").pre_name(), Some("beta"));
        assert_eq!(beta("beta.2").pre_number(), Some(2));
        assert_eq!(beta("dev").pre_number(), None);
        assert_eq!(beta("rc.0").pre_number(), Some(0));
        assert_eq!(beta("beta.2").release(), Version::new(0, 5, 0));
        assert!(beta("dev").is_pre_release() && !Version::new(1, 0, 0).is_pre_release());
        assert_eq!(beta("beta.2").to_string(), "0.5.0-beta.2");
        assert_eq!(format!("{:?}", beta("dev")), "Version(0.5.0-dev)");
        assert_eq!(Version::parse("0.5.0-beta.2+x4"), Some(beta("beta.2")));
        assert_eq!(Version::parse("1.2.3+abc"), Some(Version::new(1, 2, 3)));
        assert_eq!(Version::parse(" 1.2 "), Some(Version::new(1, 2, 0)));
        assert_eq!(beta("rc.8190").pre_number(), Some(8190));
    }

    #[test]
    fn what_is_no_version_says_why() {
        for bad in [
            "1.2-beta",
            "-1.2.3",
            "1.2.3.4",
            "x",
            "1.2.3+",
            "1.2.3+a..b",
            "1.x.3",
        ] {
            assert_eq!(Version::try_parse(bad), Err(VersionError::Numbers), "{bad}");
        }
        // Pre-releases a version does not hold.
        for bad in [
            "1.2.3-",
            "1.2.3-b c",
            "1.2.3-beta..1",
            "1.2.3-beta.01",
            "1.2.3-beta.8191",
            "1.2.3-beta.1.2",
            "1.2.3-beta.x",
            "1.2.3-gamma",
            "1.2.3-2",
            "1.2.3-Beta.1",
            "1.2.3-alpha.1.5",
        ] {
            assert_eq!(
                Version::try_parse(bad),
                Err(VersionError::PreRelease),
                "{bad}"
            );
            assert_eq!(Version::parse(bad), None);
        }
        // The text of the refusal names what is taken.
        let text = VersionError::PreRelease.to_string();
        assert!(text.contains("alpha, beta, dev, pre or rc"), "{text}");
    }

    #[test]
    fn versions_are_ordered_as_semver_orders_them() {
        // SemVer's own example (the names it uses that a version holds),
        // from first to last.
        let order = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.2",
            "1.0.0-alpha.10",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-dev",
            "1.0.0-pre.1",
            "1.0.0-rc.1",
            "1.0.0-rc.8190",
            "1.0.0",
            "1.0.1-dev",
            "1.0.1",
            "1.1.0-alpha",
            "2.0.0-rc.1",
        ];
        for pair in order.windows(2) {
            let (a, b) = (v(pair[0]), v(pair[1]));
            assert!(a < b, "{a} < {b}");
        }
        // Build metadata orders nothing.
        assert_eq!(Version::parse("1.0.0+a"), Version::parse("1.0.0+b"));
    }

    #[test]
    fn the_package_version_is_the_crates() {
        assert_eq!(crate::package_version!(), SDK_VERSION);
        assert_eq!(Version::number("12"), 12);
        assert_eq!(Version::number(""), 0);
        assert_eq!(
            Version::of_package("1", "2", "3", ""),
            Version::new(1, 2, 3)
        );
        assert_eq!(
            Version::of_package("0", "5", "0", "beta.2"),
            v("0.5.0-beta.2")
        );
    }

    #[test]
    fn parses_short_forms() {
        assert_eq!(Version::parse("2"), Some(Version::new(2, 0, 0)));
        assert_eq!(Version::parse("0.3"), Some(Version::new(0, 3, 0)));
    }

    #[test]
    fn semver_compatibility() {
        let v = Version::new(1, 4, 2);
        assert!(v.satisfies(&Version::new(1, 0, 0)));
        assert!(v.satisfies(&Version::new(1, 4, 2)));
        assert!(!v.satisfies(&Version::new(1, 5, 0)));
        assert!(!v.satisfies(&Version::new(2, 0, 0)));

        let pre = Version::new(0, 2, 5);
        assert!(pre.satisfies(&Version::new(0, 2, 0)));
        assert!(!pre.satisfies(&Version::new(0, 1, 0)));
    }

    #[test]
    fn a_pre_release_has_its_versions_interface() {
        let beta = v("0.5.0-beta.1");
        assert!(beta.satisfies(&Version::new(0, 5, 0)));
        assert!(Version::new(0, 5, 0).satisfies(&beta));
        assert!(!beta.satisfies(&Version::new(0, 4, 0)));
    }
}
