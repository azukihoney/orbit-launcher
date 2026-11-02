//! Synthetic mouse samples for owned review-test windows only. Absent in normal builds.
use std::cell::Cell;

#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub buttons: u8,
    pub inside: bool,
    pub tray: bool,
}
thread_local! {
    static SAMPLE: Cell<Option<Sample>> = Cell::new(
        (std::env::var("ORBIT_OUTSIDE_CLICK_TEST").as_deref() == Ok("1"))
            .then_some(Sample::default())
    );
}
pub fn sample() -> Option<Sample> {
    SAMPLE.with(Cell::get)
}
pub fn set(buttons: u8, flags: isize) {
    if sample().is_some() {
        SAMPLE.with(|s| {
            s.set(Some(Sample {
                buttons: buttons & 31,
                inside: flags & 1 != 0,
                tray: flags & 2 != 0,
            }))
        });
    }
}
