use super::{Line, Messages, style};
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    formatting::Arg,
    services,
    settings::{self, SettingValue},
};
struct Row {
    id: String,
    shown: u64,
    busy: u64,
    refreshes: u32,
    requests: u32,
    refused: u32,
}
fn rows(host: &Host<'_>) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<Row> = Vec::new();
    services::for_each_plugin_usage(host, &mut |p| {
        let row = Row {
            id: String::from(p.plugin),
            shown: p.shown_ms,
            busy: p.busy_us,
            refreshes: p.fast.saturating_add(p.partial).saturating_add(p.full),
            requests: p.requests,
            refused: p.refused,
        };
        if let Some(old) = rows.iter_mut().find(|r| r.id == p.plugin) {
            *old = row;
        } else {
            rows.push(row);
        }
    })?;
    Ok(rows)
}
fn name(host: &Host<'_>, m: &mut Messages, id: &str) -> Result<String, Error> {
    let mut name = String::new();
    match services::plugin_name(host, id, |text| name.push_str(text)) {
        Ok(()) => Ok(name),
        Err(Error::NotFound) if id == "core" => m.copy(host, "battery.core"),
        Err(Error::NotFound) => Ok(String::from(id)),
        Err(error) => Err(error),
    }
}
pub(super) fn append(
    host: &Host<'_>,
    m: &mut Messages,
    lines: &mut Vec<Line>,
) -> Result<(), Error> {
    let usage = rows(host)?;
    if style::boolean(host, "core.developer")? && style::boolean(host, "battery.by-plugin")? {
        let mut shown: Vec<_> = usage.iter().filter(|p| p.shown >= 60_000).collect();
        shown.sort_by_key(|p| core::cmp::Reverse(p.shown));
        if !shown.is_empty() {
            lines.push(Line::Heading(m.copy(host, "battery.by-screen")?));
            for p in shown.into_iter().take(4) {
                let duration = m.duration(host, p.shown)?;
                let text = m.format(
                    host,
                    "battery.shown-refreshes",
                    &[
                        Arg::Count(i64::from(p.refreshes)),
                        Arg::Text {
                            name: "time",
                            value: &duration,
                        },
                        Arg::Int {
                            name: "count",
                            value: i64::from(p.refreshes),
                        },
                    ],
                )?;
                lines.push(Line::Value(name(host, m, &p.id)?, text));
            }
        }
        let mut busy: Vec<_> = usage.iter().filter(|p| p.busy >= 100_000).collect();
        busy.sort_by_key(|p| core::cmp::Reverse(p.busy));
        if !busy.is_empty() {
            lines.push(Line::Heading(m.copy(host, "battery.by-processor")?));
            for p in busy.into_iter().take(4) {
                lines.push(Line::Value(name(host, m, &p.id)?, m.seconds(host, p.busy)?));
            }
        }
    }
    let mut net: Vec<_> = usage
        .iter()
        .filter(|p| p.requests.saturating_add(p.refused) > 0)
        .collect();
    net.sort_by_key(|p| core::cmp::Reverse(p.requests.saturating_add(p.refused)));
    if !net.is_empty() {
        lines.push(Line::Heading(m.copy(host, "battery.network")?));
    }
    for p in net.into_iter().take(4) {
        lines.push(Line::Value(
            name(host, m, &p.id)?,
            m.format(
                host,
                "battery.requests",
                &[
                    Arg::Count(i64::from(p.requests)),
                    Arg::Int {
                        name: "count",
                        value: i64::from(p.requests),
                    },
                ],
            )?,
        ));
        let mut more = Vec::new();
        if p.refused > 0 {
            more.push(m.format(
                host,
                "battery.refused",
                &[
                    Arg::Count(i64::from(p.refused)),
                    Arg::Int {
                        name: "count",
                        value: i64::from(p.refused),
                    },
                ],
            )?);
        }
        let mut scratch = [0; 32];
        if matches!(
            settings::read_setting(host, format!("core.network.{}", p.id), &mut scratch)?,
            SettingValue::Bool(false)
        ) {
            more.push(m.copy(host, "battery.network-off")?);
        } else {
            let choice = style::setting_text(host, &format!("core.saving.{}", p.id))?;
            if matches!(choice.as_str(), "core.saving.normal" | "core.saving.manual") {
                more.push(m.copy(host, &choice)?);
            }
        }
        if !more.is_empty() {
            lines.push(Line::Message(more.join(" · ")));
        }
        let mut note = String::new();
        match services::work_note(host, &p.id, |_, text| note.push_str(text)) {
            Ok(()) => lines.push(Line::Message(note)),
            Err(Error::NotFound) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
