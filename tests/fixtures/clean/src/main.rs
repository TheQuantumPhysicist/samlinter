mod shape;

use shape::Shape;

fn main() {
    let square = Shape::Square { side: 2 };
    println!("{} {}", square.area(), shape::is_square(&square));
}
