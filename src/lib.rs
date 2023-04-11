pub mod config;
pub mod discover;
pub mod route;
pub mod serve;

pub use serve::serve;

#[cfg(test)]
pub(crate) mod test_util {
    pub(crate) fn trace_init() {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::TRACE)
            .try_init();
    }
}
