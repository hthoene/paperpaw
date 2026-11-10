use paperpaw_sdk::Contribution;
use paperpaw_sdk::native::declare::{
    Declarations, DeclareError, EditorDecl, ExtraDecl, GroupDecl, Kind, SpecDecl,
};
use paperpaw_sdk::{Editor, Level, SettingGroupSpec, SettingKind, SettingSpec, SettingValue};

#[derive(Debug)]
pub enum ConversionError {
    Declaration(DeclareError),
    Unsupported,
}

impl From<DeclareError> for ConversionError {
    fn from(error: DeclareError) -> Self {
        Self::Declaration(error)
    }
}

impl core::fmt::Display for ConversionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Declaration(error) => error.fmt(formatter),
            Self::Unsupported => formatter.write_str("unsupported fixture declaration"),
        }
    }
}

fn relative<'a>(plugin: &str, id: &'a str) -> Result<&'a str, ConversionError> {
    id.strip_prefix(plugin)
        .and_then(|rest| rest.strip_prefix('.'))
        .filter(|rest| !rest.is_empty())
        .ok_or(ConversionError::Unsupported)
}

fn text(value: &SettingValue) -> Result<&str, ConversionError> {
    match value {
        SettingValue::Text(value) => Ok(value.as_ref()),
        _ => Err(DeclareError::Default.into()),
    }
}

fn extra<'a>(
    plugin: &str,
    name: &'a str,
    spec: &'a SettingSpec,
) -> Result<Option<ExtraDecl<'a>>, ConversionError> {
    let source = spec.extra();
    if source.check.is_some() {
        return Err(ConversionError::Unsupported);
    }
    let mut result = ExtraDecl::new(name);
    result.unit = source.unit.as_deref().unwrap_or_default();
    result.section = source.section.as_deref().unwrap_or_default();
    result.follows = source.follows.as_ref().map_or("", |id| id.as_str());
    result.shown_if = source
        .shown_if
        .as_ref()
        .map(|(id, value)| (id.as_str(), *value));
    result.editor = match &source.editor {
        Some(Editor::Screen(id)) => Some(EditorDecl::Screen(relative(plugin, id.as_str())?)),
        Some(Editor::Slot(id)) => Some(EditorDecl::Slot(id.as_str())),
        None => None,
        Some(_) => return Err(ConversionError::Unsupported),
    };
    result.step = i32::try_from(source.step).map_err(|_| DeclareError::Range)?;
    result.slider = source.slider;
    result.section_of_plugin = source.section_of_plugin;
    result.keeps_unknown = source.keeps_unknown;
    Ok((result != ExtraDecl::new(name)).then_some(result))
}

fn add_spec<F: FnMut(&[u8]) -> Result<(), i32>>(
    builder: &mut Declarations<F>,
    plugin: &str,
    spec: &SettingSpec,
) -> Result<(), ConversionError> {
    let choices: Vec<&str>;
    let kind = match (&spec.kind, &spec.default) {
        (SettingKind::Bool, SettingValue::Bool(value)) => Kind::Bool(*value),
        (SettingKind::Int { min, max }, SettingValue::Int(default)) => Kind::Int {
            default: *default,
            min: i32::try_from(*min).map_err(|_| DeclareError::Range)?,
            max: i32::try_from(*max).map_err(|_| DeclareError::Range)?,
        },
        (SettingKind::Choice(values), default) => {
            choices = values.iter().collect();
            Kind::Choice {
                default: text(default)?,
                choices: &choices,
            }
        }
        (SettingKind::Text, default) => Kind::Text(text(default)?),
        (SettingKind::Language, default) => Kind::Language(text(default)?),
        (SettingKind::List, default) => Kind::List(text(default)?),
        (SettingKind::Extended { .. }, _) => return Err(ConversionError::Unsupported),
        _ => return Err(DeclareError::Default.into()),
    };
    let name = relative(plugin, spec.id.as_str())?;
    let mut declaration = SpecDecl::new(name, kind, spec.group.as_str(), &spec.title);
    declaration.help = spec.description.as_ref().map_or("", |value| value.as_str());
    declaration.scoped = spec.scoped;
    declaration.guarded = spec.guarded;
    declaration.kept_on_reset = spec.kept_on_reset;
    declaration.secret = spec.extra().secret;
    declaration.advanced = spec.level == Level::Advanced;
    let extra = extra(plugin, name, spec)?;
    builder.spec(declaration)?;
    if let Some(extra) = extra {
        builder.extra(extra)?;
    }
    Ok(())
}

fn add_group<F: FnMut(&[u8]) -> Result<(), i32>>(
    builder: &mut Declarations<F>,
    plugin: &str,
    group: &SettingGroupSpec,
) -> Result<(), ConversionError> {
    let mut declaration = GroupDecl::new(relative(plugin, group.id.as_str())?, &group.title);
    declaration.order = group.order;
    declaration.page = group
        .page
        .as_ref()
        .map(|id| relative(plugin, id.as_str()))
        .transpose()?
        .unwrap_or("");
    declaration.parent = group.parent.as_ref().map_or("", |id| id.as_str());
    builder.group(declaration)?;
    Ok(())
}

pub fn submit<F: FnMut(&[u8]) -> Result<(), i32>>(
    plugin: &str,
    contributions: &[Contribution],
    builder: &mut Declarations<F>,
) -> Result<(), ConversionError> {
    for contribution in contributions {
        match contribution {
            Contribution::Setting(spec) => add_spec(builder, plugin, spec)?,
            Contribution::FixedSettings(specs) => {
                for spec in *specs {
                    add_spec(builder, plugin, spec)?;
                }
            }
            Contribution::SettingGroup(group) => add_group(builder, plugin, group)?,
            _ => {}
        }
    }
    builder.submit()?;
    Ok(())
}
