/// - Runs its closure when dropped, so cleanup runs even when an assertion fails.
/// - Shared by the workspace tests and, through `#[path]`, by `tests/cli.rs`.
pub struct OnDrop<F: FnMut()>(pub F);

impl<F: FnMut()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}
