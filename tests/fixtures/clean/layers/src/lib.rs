/// Builds the log layer each binary installs; depending on a subscriber crate is fine.
pub fn layer() -> tracing_subscriber::fmt::Layer<tracing_subscriber::Registry> {
    tracing_subscriber::fmt::layer()
}
