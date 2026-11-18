use super::{Line, Messages, style};
use crate::mem;
use alloc::{string::String, vec::Vec};
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
/// Select only the four displayed rows, preserving input order for equal scores.
/// Stable slice sorting pulls a >4 KiB driftsort frame into the X4 image.
fn top_four(rows: &[Row], minimum: u64, score: impl Fn(&Row) -> u64) -> [Option<&Row>; 4] {
    let mut best: [Option<&Row>; 4] = [None; 4];
    for row in rows {
        let value = score(row);
        if value < minimum {
            continue;
        }
        if let Some(at) = best
            .iter()
            .position(|old| old.is_none_or(|old| score(old) < value))
        {
            for index in (at + 1..best.len()).rev() {
                best[index] = best[index - 1];
            }
            best[at] = Some(row);
        }
    }
    best
}

fn rows(host: &Host<'_>) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<Row> = Vec::new();
    let mut failed = None;
    services::for_each_plugin_usage(host, &mut |p| {
        if failed.is_some() {
            return;
        }
        if let Err(error) = note(&mut rows, p) {
            failed = Some(error);
        }
    })?;
    match failed {
        Some(error) => Err(error),
        None => Ok(rows),
    }
}
fn note(rows: &mut Vec<Row>, p: &services::ServicePluginUsage<'_>) -> Result<(), Error> {
    let row = Row {
        id: mem::string(p.plugin)?,
        shown: p.shown_ms,
        busy: p.busy_us,
        refreshes: p.fast.saturating_add(p.partial).saturating_add(p.full),
        requests: p.requests,
        refused: p.refused,
    };
    if let Some(old) = rows.iter_mut().find(|r| r.id == p.plugin) {
        *old = row;
        Ok(())
    } else {
        mem::push(rows, row)
    }
}
fn name(host: &Host<'_>, m: &mut Messages, id: &str) -> Result<String, Error> {
    let mut name = String::new();
    let mut failed = None;
    let found = services::plugin_name(host, id, |text| {
        if failed.is_none() {
            failed = mem::push_str(&mut name, text).err();
        }
    });
    if let Some(error) = failed {
        return Err(error);
    }
    match found {
        Ok(()) => Ok(name),
        Err(Error::NotFound) if id == "core" => m.copy(host, "battery.core"),
        Err(Error::NotFound) => mem::string(id),
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
        let shown = top_four(&usage, 60_000, |p| p.shown);
        if shown[0].is_some() {
            mem::push(lines, Line::Heading(m.copy(host, "battery.by-screen")?))?;
            for p in shown.into_iter().flatten() {
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
                mem::push(lines, Line::Value(name(host, m, &p.id)?, text))?;
            }
        }
        let busy = top_four(&usage, 100_000, |p| p.busy);
        if busy[0].is_some() {
            mem::push(lines, Line::Heading(m.copy(host, "battery.by-processor")?))?;
            for p in busy.into_iter().flatten() {
                mem::push(
                    lines,
                    Line::Value(name(host, m, &p.id)?, m.seconds(host, p.busy)?),
                )?;
            }
        }
    }
    let net = top_four(&usage, 1, |p| {
        u64::from(p.requests.saturating_add(p.refused))
    });
    if net[0].is_some() {
        mem::push(lines, Line::Heading(m.copy(host, "battery.network")?))?;
    }
    for p in net.into_iter().flatten() {
        network_row(host, m, p, lines)?;
    }
    Ok(())
}

fn network_row(
    host: &Host<'_>,
    m: &mut Messages,
    p: &Row,
    lines: &mut Vec<Line>,
) -> Result<(), Error> {
    mem::push(
        lines,
        Line::Value(
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
        ),
    )?;
    let mut more = Vec::new();
    if p.refused > 0 {
        mem::push(
            &mut more,
            m.format(
                host,
                "battery.refused",
                &[
                    Arg::Count(i64::from(p.refused)),
                    Arg::Int {
                        name: "count",
                        value: i64::from(p.refused),
                    },
                ],
            )?,
        )?;
    }
    let mut scratch = [0; 32];
    if matches!(
        settings::read_setting(host, mem::format!("core.network.{}", p.id)?, &mut scratch)?,
        SettingValue::Bool(false)
    ) {
        mem::push(&mut more, m.copy(host, "battery.network-off")?)?;
    } else {
        let choice = style::setting_text(host, &mem::format!("core.saving.{}", p.id)?)?;
        if matches!(choice.as_str(), "core.saving.normal" | "core.saving.manual") {
            mem::push(&mut more, m.copy(host, &choice)?)?;
        }
    }
    if !more.is_empty() {
        mem::push(lines, Line::Message(mem::join(&more, " · ")?))?;
    }
    let mut note = String::new();
    let mut failed = None;
    let found = services::work_note(host, &p.id, |_, text| {
        if failed.is_none() {
            failed = mem::push_str(&mut note, text).err();
        }
    });
    if let Some(error) = failed {
        return Err(error);
    }
    match found {
        Ok(()) => mem::push(lines, Line::Message(note))?,
        Err(Error::NotFound) => {}
        Err(error) => return Err(error),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_four_preserves_stable_ties_and_thresholds() {
        let scores = [0, 30, 20, 30, 40, 30, 50, 30];
        let rows: Vec<_> = scores
            .iter()
            .enumerate()
            .map(|(index, &shown)| Row {
                id: alloc::format!("{index}"),
                shown,
                busy: 0,
                refreshes: 0,
                requests: 0,
                refused: 0,
            })
            .collect();
        let ids: Vec<_> = top_four(&rows, 20, |r| r.shown)
            .into_iter()
            .flatten()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(ids, ["6", "4", "1", "3"]);
        assert!(top_four(&rows, 51, |r| r.shown).iter().all(Option::is_none));
        assert_eq!(
            top_four(&rows, 40, |r| r.shown)
                .into_iter()
                .flatten()
                .count(),
            2
        );
        assert!(top_four(&[], 0, |r| r.shown).iter().all(Option::is_none));
    }
}
