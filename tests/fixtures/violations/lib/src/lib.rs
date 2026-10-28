#[cfg_attr(samlinter, expect(em_dash, reason = "fixture: em_dash cannot be exempted"))]
pub fn answer() -> u8 {
    42
}
