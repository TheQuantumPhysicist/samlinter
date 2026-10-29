/// A package with a binary target may install a subscriber anywhere.
pub fn install() {
    tracing_subscriber::fmt().init();
}
