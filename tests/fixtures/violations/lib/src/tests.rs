fn logs() {
    // tests may install a subscriber
    let _ = tracing_subscriber::fmt::try_init();
}
