#![allow(opaque_hidden_inferred_bound)]
pub mod config;
pub mod discover;
pub mod http;
pub mod route;
pub mod serve;
// pub mod svc;
pub use linkerd_app_core::svc;

pub use serve::serve;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Proxy<S> {
    config: Arc<config::Config>,
    stack: svc::Stack<S>,
}

impl<S> Proxy<S> {
    #[must_use]
    pub fn new(config: Arc<config::Config>, connect: S) -> Self {
        Self {
            config,
            stack: svc::stack(connect),
        }
    }

    #[must_use]
    pub fn map_stack<S2>(
        self,
        f: impl FnOnce(svc::Stack<S>, &config::Config) -> svc::Stack<S2>,
    ) -> Proxy<S2> {
        Proxy {
            stack: f(self.stack, &self.config),
            config: self.config,
        }
    }

    #[must_use]
    pub fn into_inner(self) -> S {
        self.stack.into_inner()
    }

    // #[must_use]
    // fn push<L: svc::Layer<S>>(self, layer: L) -> Proxy<L::Service> {
    //     Proxy {
    //         stack: self.stack.push(layer),
    //         config: self.config,
    //     }
    // }
}

#[cfg(test)]
pub(crate) mod test_util {
    pub(crate) fn trace_init() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::TRACE)
            .try_init();
    }
}
