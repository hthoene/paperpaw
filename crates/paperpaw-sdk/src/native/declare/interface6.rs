//! Conversions from the interface-6 settings types, for the kernel's and the tests'
//! fixtures. Not for plugins: a plugin declares with [`Declarations`] directly, so there
//! is one way to declare in the interface (hence `kernel`-gated, like `from_registrar`).

use alloc::borrow::ToOwned;

use super::{Declarations, DeclareError, EditorDecl, ExtraDecl, GroupDecl, Kind, SpecDecl};
use crate::settings::{
    Editor, Level, SettingExtra as SdkExtra, SettingGroupSpec, SettingKind, SettingSpec as SdkSpec,
    SettingValue,
};

/// `id` without `<plugin>.`: the name a record carries.
fn relative_to<'a>(plugin: &str, id: &'a str) -> Result<&'a str, DeclareError> {
    id.strip_prefix(plugin)
        .and_then(|rest| rest.strip_prefix('.'))
        .filter(|rest| !rest.is_empty())
        .ok_or(DeclareError::Unsupported(
            "an id outside the plugin's namespace",
        ))
}

fn kind_of(spec: &SdkSpec) -> Result<Kind, DeclareError> {
    let text = |value: &SettingValue| match value {
        SettingValue::Text(text) => Ok(text.as_ref().to_owned()),
        _ => Err(DeclareError::Default),
    };
    Ok(match (&spec.kind, &spec.default) {
        (SettingKind::Bool, SettingValue::Bool(v)) => Kind::Bool(*v),
        (SettingKind::Int { min, max }, SettingValue::Int(default)) => Kind::Int {
            default: *default,
            min: i32::try_from(*min).map_err(|_| DeclareError::Range)?,
            max: i32::try_from(*max).map_err(|_| DeclareError::Range)?,
        },
        (SettingKind::Choice(choices), default) => Kind::Choice {
            default: text(default)?,
            choices: choices.iter().map(ToOwned::to_owned).collect(),
        },
        (SettingKind::Text, default) => Kind::Text(text(default)?),
        (SettingKind::Language, default) => Kind::Language(text(default)?),
        (SettingKind::List, default) => Kind::List(text(default)?),
        (SettingKind::Extended { .. }, _) => {
            return Err(DeclareError::Unsupported("an extended setting kind"));
        }
        _ => return Err(DeclareError::Default),
    })
}

fn editor_of(plugin: &str, editor: &Editor) -> Result<EditorDecl, DeclareError> {
    Ok(match editor {
        Editor::Screen(id) => EditorDecl::Screen(relative_to(plugin, id.as_str())?.to_owned()),
        Editor::Slot(id) => EditorDecl::Slot(id.as_str().to_owned()),
    })
}

/// The extra of an interface-6 spec, if it sets anything the extra record holds.
fn extra_of(plugin: &str, name: &str, extra: &SdkExtra) -> Result<Option<ExtraDecl>, DeclareError> {
    if extra.check.is_some() {
        return Err(DeclareError::Unsupported("a value check (a callback)"));
    }
    let mut out = ExtraDecl::new(name);
    out.unit = extra.unit.as_deref().unwrap_or_default().to_owned();
    out.section = extra.section.as_deref().unwrap_or_default().to_owned();
    out.follows = extra
        .follows
        .as_ref()
        .map(|id| id.as_str().to_owned())
        .unwrap_or_default();
    out.shown_if = extra
        .shown_if
        .as_ref()
        .map(|(id, value)| (id.as_str().to_owned(), (*value).to_owned()));
    out.editor = extra
        .editor
        .as_ref()
        .map(|e| editor_of(plugin, e))
        .transpose()?;
    out.step = i32::try_from(extra.step).map_err(|_| DeclareError::Range)?;
    out.slider = extra.slider;
    out.section_of_plugin = extra.section_of_plugin;
    out.keeps_unknown = extra.keeps_unknown;
    Ok((out != ExtraDecl::new(name)).then_some(out))
}

impl Declarations {
    /// Adds a group of interface 6 as the plugin `plugin` registers it.
    ///
    /// # Errors
    ///
    /// The group's id is not `<plugin>.<name>`, or [`Declarations::group`]
    /// refuses it.
    pub fn from_group(
        &mut self,
        plugin: &str,
        group: &SettingGroupSpec,
    ) -> Result<&mut Self, DeclareError> {
        let mut out = GroupDecl::new(relative_to(plugin, group.id.as_str())?, &group.title);
        out.order = group.order;
        if let Some(page) = &group.page {
            out.page = relative_to(plugin, page.as_str())?.to_owned();
        }
        out.parent = group
            .parent
            .as_ref()
            .map(|id| id.as_str().to_owned())
            .unwrap_or_default();
        self.group(out)
    }

    /// Adds a setting of interface 6 as the plugin `plugin` registers it: the
    /// `setting.spec`, and a `setting.extra` after it when it sets any of what
    /// that holds. A secret moves into the spec's flags.
    ///
    /// # Errors
    ///
    /// The id is not `<plugin>.<name>`, the kind is extended, a value check is
    /// set, or [`Declarations::spec`] / [`Declarations::extra`] refuses it.
    pub fn from_spec(&mut self, plugin: &str, spec: &SdkSpec) -> Result<&mut Self, DeclareError> {
        let name = relative_to(plugin, spec.id.as_str())?;
        let extra = spec.extra();
        let mut out = SpecDecl::new(name, kind_of(spec)?, spec.group.as_str(), &spec.title);
        out.help = spec
            .description
            .as_ref()
            .map(|d| d.as_str().to_owned())
            .unwrap_or_default();
        out.scoped = spec.scoped;
        out.guarded = spec.guarded;
        out.kept_on_reset = spec.kept_on_reset;
        out.secret = extra.secret;
        out.advanced = spec.level == Level::Advanced;
        let extra = extra_of(plugin, name, extra)?;
        self.spec(out)?;
        match extra {
            Some(extra) => self.extra(extra),
            None => Ok(self),
        }
    }
}

impl Declarations {
    /// The settings and groups an interface-6 plugin registered, as records in
    /// the order it registered them: for the kernel's and the tests' fixtures
    /// (a plugin ported to interface 7 declares them with the other methods).
    ///
    /// # Errors
    ///
    /// The first setting or group that [`Declarations::from_spec`] or
    /// [`Declarations::from_group`] refuses.
    pub fn from_registrar(plugin: &str, registrar: crate::Registrar) -> Result<Self, DeclareError> {
        use crate::Contribution;
        let mut out = Self::new();
        for contribution in registrar.into_contributions() {
            match contribution {
                Contribution::Setting(spec) => {
                    out.from_spec(plugin, &spec)?;
                }
                Contribution::FixedSettings(list) => {
                    for spec in list {
                        out.from_spec(plugin, spec)?;
                    }
                }
                Contribution::SettingGroup(group) => {
                    out.from_group(plugin, &group)?;
                }
                _ => {}
            }
        }
        Ok(out)
    }
}
