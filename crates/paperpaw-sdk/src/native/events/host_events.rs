//! The host's own events: lifecycle, pages, screens, actions, registry and plugin requests.
use super::{
    Error, Event, PluginRequestId, ReadingView, RegistryChanges, RequestedPlugins, TypedText, wire,
};

pub(super) mod tick {
    use super::{Error, Event, wire};
    #[inline(always)]
    pub(in crate::native::events) fn inline<'a>(
        _raw: wire::RawRecord<'a>,
        bytes: &'a [u8],
    ) -> Result<Event<'a>, Error> {
        // The whole bytes: this event is read without the generic record decoder.
        wire::tick_decode(bytes)
            .map(|now_ms| Event::Tick { now_ms })
            .map_err(Error::from_status)
    }
    #[inline(never)]
    pub(in crate::native::events) fn alone<'a>(
        raw: wire::RawRecord<'a>,
        bytes: &'a [u8],
    ) -> Result<Event<'a>, Error> {
        inline(raw, bytes)
    }
}

event_fn!(stage_call, wire::StageCall<'_>, |r| Event::StageCall {
    stage: r.stage,
    schema: r.schema,
    payload: r.payload,
});
event_fn!(started, wire::EventStarted, |r| Event::Started {
    now_ms: r.now_ms
});
event_fn!(resume, wire::EventResume, |r| Event::Resume {
    now_ms: r.now_ms
});
event_fn!(suspend, wire::EventSuspend, |r| Event::Suspend {
    now_ms: r.now_ms
});
event_fn!(stopping, wire::EventStopping, |r| Event::Stopping {
    now_ms: r.now_ms
});
event_fn!(battery_measured, wire::EventBatteryMeasured, |r| {
    Event::BatteryMeasured { now_ms: r.now_ms }
});
event_fn!(page_turned, wire::EventPageTurned<'_>, |r| {
    Event::PageTurned(
        ReadingView {
            now_ms: r.now_ms,
            section: r.section,
            offset: r.offset,
            insert: r.insert.unwrap_or(0),
            progress_ppm: r.progress_ppm,
            path: r.path,
            title: r.title,
            chapter: r.chapter,
            page: r.page,
            pages: r.pages,
            chapter_left: r.chapter_left,
        }
        .validate()?,
    )
});
event_fn!(screen_shown, wire::EventScreenShown<'_>, |r| {
    Event::ScreenShown {
        now_ms: r.now_ms,
        screen: r.screen,
        width: r.width,
        height: r.height,
        instance: r
            .instance
            .map(|value| crate::native::ui::Instance::new(value).ok_or(Error::Invalid))
            .transpose()?,
        last_input: match r.last_input {
            0 => None,
            1 => Some(crate::keys::InputMode::Buttons),
            2 => Some(crate::keys::InputMode::Touch),
            3 => Some(crate::keys::InputMode::Pen),
            _ => return Err(Error::Invalid),
        },
    }
});
event_fn!(action_run, wire::EventActionRun<'_>, |r| {
    // Only bit 0 (`background`) is defined; a decoder refuses the others.
    if r.flags & !1 != 0 {
        return Err(Error::Invalid);
    }
    Event::ActionRun {
        now_ms: r.now_ms,
        action: r.action,
        background: r.flags & 1 != 0,
    }
});
event_fn!(text_typed, wire::EventTextTyped<'_>, |r| {
    // Only bit 0 (`done`) is defined; a decoder refuses the others.
    if r.flags & !1 != 0 {
        return Err(Error::Invalid);
    }
    Event::TextTyped {
        now_ms: r.now_ms,
        text: TypedText::new(r.text),
        done: r.flags & 1 != 0,
    }
});
event_fn!(registry_changed, wire::EventRegistryChanged, |r| {
    Event::RegistryChanged {
        now_ms: r.now_ms,
        what: RegistryChanges::from_bits(r.what),
    }
});
event_fn!(preview_changed, wire::EventPreviewChanged, |r| {
    Event::PreviewChanged { now_ms: r.now_ms }
});
event_fn!(plugins_requested, wire::EventPluginsRequested<'_>, |r| {
    Event::PluginsRequested {
        now_ms: r.now_ms,
        request: PluginRequestId(r.request),
        by: r.by,
        plugins: RequestedPlugins::decode(r.plugins)?,
    }
});
event_fn!(plugin_answered, wire::EventPluginAnswered<'_>, |r| {
    use crate::native::commands::PluginOutcome;
    Event::PluginAnswered {
        now_ms: r.now_ms,
        request: PluginRequestId(r.request),
        plugin: r.plugin,
        outcome: match r.outcome {
            0 => PluginOutcome::Installed,
            1 => PluginOutcome::Declined,
            3 => PluginOutcome::NotFound,
            _ => PluginOutcome::Failed,
        },
    }
});
