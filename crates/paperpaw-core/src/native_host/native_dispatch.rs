//! Retained native owners behind the screen router's synchronous dispatch API.
use super::{
    Error, FlatHost, flat::FlatDepth, native_screen::Dispatch, registrations::DrawFrame,
    retained::Retained, shell,
};
use crate::abi7::core_frame::CoreFrame;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{self, Services};
use paperpaw_sdk::Env;

pub(crate) struct NativeDispatch<S: Services + 'static, M: 'static> {
    instance: Retained<S, M>,
    depth: FlatDepth,
    host: FlatHost,
    report: fn(&S, u32, i32),
}

impl<S: Services + 'static, M: 'static> NativeDispatch<S, M> {
    pub(crate) fn new(
        instance: Retained<S, M>,
        depth: FlatDepth,
        host: FlatHost,
        report: fn(&S, u32, i32),
    ) -> Self {
        Self {
            instance,
            depth,
            host,
            report,
        }
    }

    fn frame<'a>(&'a self, env: &'a Env<'a>) -> CoreFrame<'a> {
        CoreFrame {
            env,
            meter: &self.host.meter,
            font_removals: &self.host.font_removals,
            usage: env.usage(),
            plugins: env.plugins(),
            work: env.work(),
        }
    }

    fn invoke(
        &self,
        token: u32,
        env: &Env<'_>,
        invocation: shell::TokenInvocation<'_>,
    ) -> Result<i32, i32> {
        let batch = self.instance.batch().map_err(|e| status(e.into()))?;
        shell::dispatch_token(
            batch,
            token,
            self.depth.as_depth(),
            &self.frame(env),
            invocation,
        )
        .map_err(status)
    }
}

impl<S: Services + 'static, M: 'static> Dispatch for NativeDispatch<S, M> {
    fn event(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.invoke(token, env, shell::TokenInvocation::Event(bytes))
    }

    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.invoke(token, env, shell::TokenInvocation::Input(bytes))
    }

    fn draw(
        &self,
        token: u32,
        frame: &DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        let batch = self.instance.batch().map_err(|e| status(e.into()))?;
        shell::dispatch_token_draw(
            batch,
            token,
            self.depth.as_depth(),
            &self.frame(env),
            frame,
            canvas,
        )
        .map_err(status)
    }

    fn dropped(&self, token: u32, status: i32) {
        (self.report)(
            &self.instance.host().owner().context().services,
            token,
            status,
        );
    }
}

fn status(error: shell::Error) -> i32 {
    match error {
        shell::Error::Status(code) | shell::Error::Factory(code) => code,
        shell::Error::Allocation | shell::Error::Lifecycle(Error::Memory) => {
            host_glue::ERR_NO_MEMORY
        }
        shell::Error::Lifecycle(Error::Busy | Error::Depth) => host_glue::ERR_BUSY,
        shell::Error::Lifecycle(Error::Capacity) => host_glue::ERR_NO_SPACE,
        shell::Error::Lifecycle(Error::State) | shell::Error::Attempt => host_glue::ERR_STATE,
        _ => host_glue::ERR_INVALID,
    }
}
