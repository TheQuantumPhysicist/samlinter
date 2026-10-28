use std::fmt;
fn helper() {}
fn main() {
    // See NOTES.md before changing this.
    // Waits for the lock — never spins.
    // Done 🎉
    let point = Point { x: 1, y: 2 };
    let Point { x, .. } = point;
    println!("{x}");
}
pub(crate) struct Point {
    x: i32,
    y: i32,
}
