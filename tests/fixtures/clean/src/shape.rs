pub enum Shape {
    Square { side: u32 },
    Circle,
}

impl Shape {
    pub const fn area(&self) -> u32 {
        match self {
            Self::Square { side } => side * side,
            Self::Circle => 0,
        }
    }

    pub const fn corners(&self) -> u32 {
        match self {
            Self::Square { side: _ } => 4,
            Self::Circle => 0,
        }
    }
}

pub fn is_square(shape: &Shape) -> bool {
    match shape {
        #[cfg_attr(samlinter, expect(rest_pattern, reason = "fixture: a fulfilled exemption"))]
        Shape::Square { .. } => true,
        Shape::Circle => false,
    }
}

pub fn first(values: &[u8]) -> Option<u8> {
    match values {
        [first, ..] => Some(*first),
        [] => None,
    }
}
