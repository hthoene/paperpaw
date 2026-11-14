//! Where a plugin's settings show: a group of its own on the settings'
//! main page names its order and an icon, and a widget's settings are in
//! the widget's own group, opened where the widget is edited, not in one
//! of the main page.

use paperpaw_core::Kernel;

use crate::report::Finding;

/// The findings on the setting groups of the plugins `ours` names.
pub(super) fn groups(kernel: &Kernel, ours: &dyn Fn(&str) -> Option<String>) -> Vec<Finding> {
    let settings = kernel.settings();
    let widgets: Vec<String> = kernel
        .widgets()
        .everywhere()
        .into_iter()
        .map(|(id, _, _)| id.to_string())
        .collect();
    let mut findings = Vec::new();
    for group in settings.groups() {
        let id = group.id.as_str();
        let Some(plugin) = ours(id) else {
            continue;
        };
        // A widget's own group is not on the main page, nor a part of
        // another group (shown on that group's page).
        if widgets.iter().any(|widget| widget == id) || group.parent.is_some() {
            continue;
        }
        if group.order == 0 {
            findings.push(
                Finding::warning(
                    "settings.group-order",
                    id,
                    "The settings group names no order, so the settings list it at the end of \
                     Customise; give it one of its section (`SettingGroupSpec::order`: books 1 to \
                     49, the device 51 to 199, customising from 201).",
                )
                .plugin(&plugin),
            );
        }
        if kernel.menus().icon(id).is_none() {
            findings.push(
                Finding::warning(
                    "settings.group-icon",
                    id,
                    "The settings group has no icon, so the settings show a plugin's; give it \
                     one (`Registrar::add_icon` with the group's id).",
                )
                .plugin(&plugin),
            );
        }
        // A widget plugin that keeps settings in a group of the main page,
        // and none in its widgets' own.
        let own_widgets: Vec<&str> = widgets
            .iter()
            .map(String::as_str)
            .filter(|widget| ours(widget).as_deref() == Some(plugin.as_str()))
            .collect();
        let in_own = own_widgets.iter().any(|widget| {
            settings
                .specs()
                .any(|spec| settings.group_of(spec).as_str() == *widget)
        });
        if !own_widgets.is_empty() && !in_own {
            findings.push(
                Finding::warning(
                    "settings.widget-group",
                    id,
                    format!(
                        "A widget's settings belong in its own group \
                         (`widgets::settings_group(&{})`), opened where the widget is edited; \
                         a group of the main page shows them among PaperPaw's own settings.",
                        own_widgets[0]
                    ),
                )
                .plugin(&plugin),
            );
        }
    }
    // A widget's setting kept in another group than the widget's own.
    for widget in &widgets {
        let Some(plugin) = ours(widget.as_str()) else {
            continue;
        };
        let prefix = format!("{widget}.");
        for spec in settings.specs() {
            let group = settings.group_of(spec);
            if spec.id.as_str().starts_with(&prefix)
                && group.as_str() != widget
                && !widgets.iter().any(|w| w == group.as_str())
            {
                findings.push(
                    Finding::warning(
                        "settings.widget-group",
                        spec.id.to_string(),
                        format!(
                            "A setting of the widget {widget} sits in the group {group}: give it \
                             the widget's own (`widgets::settings_group(&{widget})`), opened where \
                             the widget is edited."
                        ),
                    )
                    .plugin(&plugin),
                );
            }
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use paperpaw_sdk::widgets::{Drawn, WidgetSpec};
    use paperpaw_sdk::{
        Id, Manifest, MemoryStorage, Plugin, RegisterError, Registrar, SettingGroupSpec,
        SettingSpec, Version,
    };

    use super::*;

    /// A widget plugin as the workshop's duck: a group of the main page
    /// with no order and no icon, its widget's settings in it.
    struct Duck;

    static SETTINGS: [SettingSpec; 1] = [SettingSpec::switch(
        "duck.widget.big",
        "Big",
        false,
        Id::new_static("duck.group"),
    )];

    impl Plugin for Duck {
        fn manifest(&self) -> Manifest {
            Manifest::new(Id::new_static("duck"), "Duck", Version::new(0, 1, 0))
                .uses(paperpaw_sdk::Uses::NOTHING)
        }

        fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
            r.add_setting_group(SettingGroupSpec::new(Id::new_static("duck.group"), "Duck"))?;
            r.add_widget(WidgetSpec::drawn(
                Id::new_static("duck.widget"),
                "Duck",
                Drawn::new(|_, _| 10, |_, _, _| {}),
            ))?;
            r.add_settings(&SETTINGS)
        }
    }

    #[test]
    fn a_widget_plugins_group_without_order_or_icon_is_named() {
        let device = paperpaw_emulator::profiles::find("xteink-x4-pro")
            .unwrap()
            .device;
        let kernel = Kernel::builder(device)
            .storage(MemoryStorage::new())
            .plugin(Duck)
            .build()
            .unwrap();
        let ours = |id: &str| id.starts_with("duck.").then(|| String::from("duck"));
        let found: Vec<(&str, String)> = groups(&kernel, &ours)
            .into_iter()
            .map(|f| (f.check, f.place))
            .collect();
        for (check, place) in [
            ("settings.group-order", "duck.group"),
            ("settings.group-icon", "duck.group"),
            ("settings.widget-group", "duck.group"),
            ("settings.widget-group", "duck.widget.big"),
        ] {
            assert!(
                found.iter().any(|(c, p)| *c == check && p == place),
                "{check} {place}: {found:?}"
            );
        }
        // PaperPaw's own groups give no finding.
        let ours = |id: &str| (!id.starts_with("duck.")).then(|| String::from("core"));
        assert!(groups(&kernel, &ours).is_empty());
    }
}
