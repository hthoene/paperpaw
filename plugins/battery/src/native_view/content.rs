//! Screen content owns its strings; borrowed service rows never escape callbacks.
use super::{Line, history, messages::Messages, style};
use crate::log::{Kind, Reading, Session};
use alloc::{format, string::String, vec::Vec};
use paperpaw_sdk::native::{Error, Host, formatting::Arg};
mod usage;

pub(super) fn lines(
    host: &Host<'_>,
    now: Reading,
    m: &mut Messages,
) -> Result<(Vec<Line>, String), Error> {
    let mut sessions = history::read(host)?;
    sessions.feed(Kind::Sample, now);
    let mut lines = Vec::new();
    lines.push(Line::Heading(m.copy(host, "battery.now")?));
    if let Some(charge) = now.percent {
        let mut value = m.format(
            host,
            "battery.percent",
            &[Arg::Int {
                name: "value",
                value: i64::from(charge),
            }],
        )?;
        if let Some(mv) = now.millivolts {
            value = format!("{value} · {}", m.volts(host, mv)?);
        }
        lines.push(Line::Value(m.copy(host, "battery.charge")?, value));
        lines.push(Line::Value(
            m.copy(host, "battery.power")?,
            m.copy(
                host,
                if now.charging {
                    "battery.charging"
                } else {
                    "battery.on-battery"
                },
            )?,
        ));
    } else {
        lines.push(Line::Message(m.copy(host, "battery.unknown")?));
    }
    if let Some(current) = &sessions.current {
        current_lines(host, m, current, &mut lines)?;
    }
    lines.push(Line::Heading(m.copy(host, "battery.so-far")?));
    let totals = sessions.totals();
    if let Some(rate) = totals.per_hour() {
        lines.push(Line::Value(
            m.copy(host, "battery.per-hour")?,
            m.percent(host, rate)?,
        ));
        if let Some(rate) = totals.per_100_pages() {
            lines.push(Line::Value(
                m.copy(host, "battery.per-100-pages")?,
                m.percent(host, rate)?,
            ));
        }
        if let Some(hours) = now.percent.and_then(|p| totals.hours_left(p)) {
            lines.push(Line::Value(
                m.copy(host, "battery.left")?,
                m.format(
                    host,
                    "battery.about-hours",
                    &[Arg::Int {
                        name: "h",
                        value: (hours + 0.5) as i64,
                    }],
                )?,
            ));
        }
    } else {
        lines.push(Line::Message(m.copy(host, "battery.too-little")?));
    }
    usage::append(host, m, &mut lines)?;
    let recent: Vec<_> = sessions.ended.iter().rev().filter(|s| s.counts()).collect();
    if !recent.is_empty() {
        lines.push(Line::Heading(m.copy(host, "battery.recent")?));
        for session in recent {
            let pages = m.format(
                host,
                "battery.pages-count",
                &[Arg::Count(i64::from(session.pages()))],
            )?;
            lines.push(Line::Value(
                format!("{} · {pages}", m.duration(host, session.millis())?),
                m.format(
                    host,
                    "battery.used-percent",
                    &[Arg::Int {
                        name: "value",
                        value: i64::from(session.used().unwrap_or(0)),
                    }],
                )?,
            ));
        }
    }
    let footer = if style::boolean(host, "core.developer")? {
        m.format(
            host,
            "battery.log-file",
            &[Arg::Text {
                name: "path",
                value: ".paperpaw/plugins/battery/log.csv",
            }],
        )?
    } else {
        String::new()
    };
    Ok((lines, footer))
}
fn current_lines(
    host: &Host<'_>,
    m: &mut Messages,
    current: &Session,
    lines: &mut Vec<Line>,
) -> Result<(), Error> {
    lines.push(Line::Heading(m.copy(host, "battery.awake")?));
    lines.push(Line::Value(
        m.copy(host, "battery.awake-for")?,
        m.duration(host, current.millis())?,
    ));
    if let (Some(from), Some(to)) = (current.first.percent, current.last.percent) {
        lines.push(Line::Value(
            m.copy(host, "battery.from-to")?,
            m.format(
                host,
                "battery.percent-from-to",
                &[
                    Arg::Int {
                        name: "from",
                        value: i64::from(from),
                    },
                    Arg::Int {
                        name: "to",
                        value: i64::from(to),
                    },
                ],
            )?,
        ));
    }
    lines.push(Line::Value(
        m.copy(host, "battery.pages")?,
        m.integer(host, i64::from(current.pages()))?,
    ));
    lines.push(Line::Value(
        m.copy(host, "battery.refreshes")?,
        m.integer(host, i64::from(current.refreshes()))?,
    ));
    Ok(())
}
