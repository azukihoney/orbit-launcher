//! One independent remainder per scrolling area. No OS or persistence.
#[derive(Default)]
pub struct Wheel {
    remainder: i32,
}
impl Wheel {
    pub fn reset(&mut self) {
        self.remainder = 0;
    }
    pub fn feed(&mut self, delta: i32) -> i32 {
        if delta == 0 {
            return 0;
        }
        if self.remainder.signum() != delta.signum() {
            self.remainder = 0;
        }
        let total = self.remainder + delta;
        self.remainder = total % 120;
        total / 120
    }
}
