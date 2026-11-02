use crate::model::Settings;
use std::collections::VecDeque;
#[derive(Default)]
pub struct Detector {
    points: VecDeque<(f64, f64, f64)>,
    blocked_until: f64,
}
impl Detector {
    pub fn reset(&mut self) {
        self.points.clear();
    }
    pub fn feed(&mut self, x: f64, y: f64, now: f64, s: &Settings) -> bool {
        if now < self.blocked_until {
            return false;
        }
        if self.points.back().is_some_and(|p| now - p.2 > 0.45) {
            self.reset();
        }
        while self.points.front().is_some_and(|p| now - p.2 > s.timeout) {
            self.points.pop_front();
        }
        if self
            .points
            .back()
            .is_some_and(|p| (x - p.0).hypot(y - p.1) < 2.)
        {
            return false;
        }
        if self.points.len() == 512 {
            self.points.pop_front();
        }
        self.points.push_back((x, y, now));
        if self.points.len() < 8 {
            return false;
        }
        let pts: Vec<_> = self.points.iter().copied().collect();
        for start in (0..pts.len() - 7).step_by(6) {
            if assess(&pts[start..], s) {
                self.reset();
                self.blocked_until = now + 1.5;
                return true;
            }
        }
        false
    }
}
fn assess(p: &[(f64, f64, f64)], s: &Settings) -> bool {
    let minx = p.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let maxx = p.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max);
    let miny = p.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let maxy = p.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let (w, h) = (maxx - minx, maxy - miny);
    if w.min(h) < s.min_radius * 2. || w.max(h) > s.max_radius * 2. || w.max(h) / w.min(h) > 3.5 {
        return false;
    }
    let (cx, cy) = ((minx + maxx) / 2., (miny + maxy) / 2.);
    let radii: Vec<_> = p.iter().map(|p| (p.0 - cx).hypot(p.1 - cy)).collect();
    let radius = radii.iter().sum::<f64>() / p.len() as f64;
    if !(s.min_radius..=s.max_radius).contains(&radius) {
        return false;
    }
    let deviation =
        (radii.iter().map(|r| (r - radius).powi(2)).sum::<f64>() / p.len() as f64).sqrt() / radius;
    if deviation > 0.65 {
        return false;
    }
    let mut winding = 0.;
    let mut travel = 0.;
    for pair in p.windows(2) {
        let a = (pair[0].1 - cy).atan2(pair[0].0 - cx);
        let b = (pair[1].1 - cy).atan2(pair[1].0 - cx);
        let change = (b - a).sin().atan2((b - a).cos());
        winding += change;
        travel += change.abs();
    }
    let direction = if winding < 0. { "ccw" } else { "cw" };
    (s.direction == "both" || s.direction == direction)
        && winding.abs() / std::f64::consts::TAU >= s.turns - 0.045
        && travel > 0.
        && winding.abs() / travel > 0.75
}
#[cfg(test)]
#[path = "gesture_tuning_tests.rs"]
mod tuning_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn human_trace_is_recognized_and_clockwise_is_rejected() {
        let value: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/hand_drawn_ccw.json")).unwrap();
        let motions = value["motions"].as_array().unwrap();
        let s = Settings {
            turns: 2.,
            timeout: 2.5,
            ..Settings::default()
        };
        let mut d = Detector::default();
        let mut hits = Vec::new();
        for p in motions {
            let (t, x, y) = (
                p[0].as_f64().unwrap(),
                p[1].as_f64().unwrap(),
                p[2].as_f64().unwrap(),
            );
            if d.feed(x, y, t, &s) {
                hits.push(t);
            }
        }
        assert_eq!(hits.len(), 4);
        for (hit, window) in hits
            .iter()
            .zip(value["gesture_windows"].as_array().unwrap())
        {
            assert!(*hit >= window[0].as_f64().unwrap() && *hit <= window[1].as_f64().unwrap());
        }
        let end = motions.last().unwrap()[0].as_f64().unwrap();
        let mut d = Detector::default();
        let mut count = 0;
        for p in motions.iter().rev() {
            count += usize::from(d.feed(
                p[1].as_f64().unwrap(),
                p[2].as_f64().unwrap(),
                end - p[0].as_f64().unwrap(),
                &s,
            ));
        }
        assert_eq!(count, 0);
    }
}
