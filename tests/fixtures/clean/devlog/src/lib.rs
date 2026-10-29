pub struct Worker;

impl Worker {
    pub const fn init(&self) {}
}

/// A subscriber crate only among the dev-dependencies leaves `.init()` calls alone.
pub fn start(worker: &Worker) {
    worker.init();
}
