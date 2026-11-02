use super::*;
use std::f64::consts::TAU;

fn settings(timeout: f64) -> Settings {
    Settings {
        timeout,
        min_radius: 100.,
        ..Settings::default()
    }
}
fn curve(n: usize, turns: f64, rx: f64, ry: f64) -> Vec<(f64, f64, f64)> {
    (0..n)
        .map(|i| {
            let a = TAU * turns * i as f64 / (n - 1) as f64;
            (rx * a.cos(), -ry * a.sin(), i as f64 * 0.01)
        })
        .collect()
}
fn seed(points: Vec<(f64, f64, f64)>) -> Detector {
    Detector {
        points: points.into(),
        blocked_until: 0.,
    }
}
fn hits(points: &[(f64, f64, f64)], s: &Settings) -> Vec<f64> {
    let mut d = Detector::default();
    points
        .iter()
        .filter_map(|&(x, y, t)| d.feed(x, y, t, s).then_some(t))
        .collect()
}

#[test] // Kills retained 24-point wait, missing minimum guard, and wrong candidate end bound.
fn tuning_seven_geometric_points_wait_but_eighth_recognizes() {
    let s = settings(1.);
    let p = curve(7, 1.5, 300., 300.);
    assert!(
        assess(&p, &s),
        "Seven-point guard must be exercised independently of shape rejection"
    );
    let mut d = Detector::default();
    for &(x, y, t) in &p {
        assert!(!d.feed(x, y, t, &s));
    }
    let angle = TAU * 1.625;
    assert!(d.feed(300. * angle.cos(), -300. * angle.sin(), 0.07, &s));
    assert_eq!(d.blocked_until, 1.57);
}

#[test] // Kills stride=1: freezes approved B8s6, without treating missed gestures as desirable.
fn tuning_six_point_stride_compatibility_not_a_quality_goal() {
    let s = settings(1.);
    let mut p = vec![(5000., 5000., 0.)];
    p.extend(
        curve(9, 1.5, 300., 300.)
            .into_iter()
            .map(|(x, y, t)| (x, y, t + 0.01)),
    );
    assert!(assess(&p[1..], &s));
    assert!(!assess(&p, &s));
    let last = p.pop().unwrap();
    assert!(!seed(p).feed(last.0, last.1, last.2, &s));
}

#[test] // Kills gap deletion and >= instead of > at 0.45, independently of timeout.
fn tuning_gap_boundary_is_between_accepted_points() {
    let s = settings(1.);
    let p = curve(7, 1.5, 300., 300.)
        .into_iter()
        .map(|(x, y, t)| (x, y, t - 0.06))
        .collect::<Vec<_>>();
    let a = TAU * 1.625;
    assert!(seed(p.clone()).feed(300. * a.cos(), -300. * a.sin(), 0.45, &s));
    let mut d = seed(p);
    assert!(!d.feed(300. * a.cos(), -300. * a.sin(), 0.450001, &s));
    assert_eq!(d.points.len(), 1);
}

#[test] // Kills timeout removal and >= at the exact timeout, without invoking the gap gate.
fn tuning_timeout_boundary_and_expiry() {
    let s = settings(0.3);
    let p = curve(7, 1.5, 300., 300.);
    let a = TAU * 1.625;
    assert!(seed(p.clone()).feed(300. * a.cos(), -300. * a.sin(), 0.3, &s));
    let mut d = seed(p);
    assert!(!d.feed(300. * a.cos(), -300. * a.sin(), 0.300001, &s));
    assert_eq!(d.points.len(), 7);
}

#[test] // Kills jitter acceptance, cap deletion, and reset clearing an external cooldown.
fn tuning_jitter_capacity_and_cooldown_remain_independent() {
    let s = settings(6.);
    let mut d = seed(vec![(0., 0., 0.)]);
    assert!(!d.feed(1.999, 0., 0.01, &s));
    assert_eq!(d.points.len(), 1);
    assert!(!d.feed(2., 0., 0.02, &s));
    assert_eq!(d.points.len(), 2);
    let mut d = seed(
        (0..512)
            .map(|i| (i as f64 * 3., 0., i as f64 * 0.01))
            .collect(),
    );
    assert!(!d.feed(1536., 0., 5.12, &s));
    assert_eq!(d.points.len(), 512);
    assert_eq!(d.points.front().unwrap().0, 3.);
    d.blocked_until = 10.;
    d.reset();
    assert!(!d.feed(0., 0., 9.999, &s));
    assert!(d.points.is_empty());
    assert!(!d.feed(0., 0., 10., &s));
    assert_eq!(d.points.len(), 1);
}

#[test] // Each rejected input passes the OTHER radius/bounds guard; catches one-sided removal.
fn tuning_shape_guards_have_separate_witnesses() {
    let s = settings(1.);
    assert!(assess(&curve(97, 2., 300., 300.), &s));
    let short = curve(97, 2., 260., 80.); // ratio=3.25, average radius >100, short side 160.
    assert!(!assess(&short, &s));
    let lower = (0..97)
        .map(|i| {
            let a = TAU * 2. * i as f64 / 96.;
            let r = if i % 12 == 0 { 100. } else { 90. };
            (r * a.cos(), -r * a.sin(), i as f64 * 0.01)
        })
        .collect::<Vec<_>>();
    assert!(!assess(&lower, &s)); // 200x200 bounds pass, average radius <100.
    assert!(!assess(&curve(97, 2., 900., 300.), &s)); // long side only; average <800.
    let corners = (0..9)
        .map(|i| {
            let (x, y) = [(800., 800.), (800., -800.), (-800., -800.), (-800., 800.)][i % 4];
            (x, y, i as f64 * 0.01)
        })
        .collect::<Vec<_>>();
    assert!(!assess(&corners, &s)); // bounds 1600x1600 pass; radius >800.
    let loose = Settings {
        min_radius: 5.,
        ..s.clone()
    };
    assert!(!assess(&curve(97, 2., 400., 100.), &loose)); // aspect alone exceeds 3.5.
    let noisy = (0..97)
        .map(|i| {
            let a = TAU * 2. * i as f64 / 96.;
            let r = if i % 2 == 0 { 10. } else { 500. };
            (r * a.cos(), -r * a.sin(), i as f64 * 0.01)
        })
        .collect::<Vec<_>>();
    assert!(!assess(&noisy, &loose)); // radial deviation >.65; all size gates pass.
    let impure = (0..145)
        .map(|i| {
            let turns = if i <= 108 {
                i as f64 / 48.
            } else {
                2.25 - (i - 108) as f64 / 48.
            };
            let a = turns * TAU;
            (300. * a.cos(), -300. * a.sin(), i as f64 * 0.01)
        })
        .collect::<Vec<_>>();
    assert!(!assess(&impure, &s)); // net 1.5, travel 3.0: purity only rejects.
}

fn synthetic(
    kind: usize,
    dt: f64,
    period: f64,
    scale: f64,
    shift: (f64, f64),
    phase: f64,
    turns: f64,
) -> Vec<(f64, f64, f64)> {
    let duration = period * turns;
    (0..=(duration / dt + 1e-9).floor() as usize)
        .map(|i| {
            let t = i as f64 * dt;
            let a = TAU * t / period + phase;
            let (x, y) = match kind {
                0 => (300. * a.cos(), -300. * a.sin()),
                1 => (360. * a.cos(), -220. * a.sin()),
                2 => (60. * a.cos(), -60. * a.sin()),
                3 => (900. * a.cos(), -900. * a.sin()),
                4 => (300. * a.sin(), 0.),
                5 => (
                    600. * t / duration - 300.,
                    120. * (2. / std::f64::consts::PI) * (3. * a).sin().asin(),
                ),
                6 => (600. * t / duration - 300., 20. * a.sin()),
                7 => (
                    500. * t / duration - 250.,
                    300. * t / duration - 150. + 30. * a.sin(),
                ),
                8 => (3. * a.sin(), 300. * (2. * a).sin()),
                9 => (6. * (7. * a).sin(), 5. * (11. * a).cos()),
                10 => (300. * a.cos(), 300. * a.sin()),
                _ => (300. * a.cos(), -300. * a.sin()),
            };
            (
                (x * scale + shift.0).round_ties_even(),
                (y * scale + shift.1).round_ties_even(),
                t,
            )
        })
        .collect()
}

#[test] // Kills the 24-point wait; freezes all 48 unique inputs underlying stage1's 144 rows.
fn tuning_synthetic_positives_preserve_a_and_add_fast_30ms() {
    for timeout in [1., 2.5] {
        for kind in 0..4 {
            for period in [0.25, 1.2] {
                for dt in [0.01, 0.016, 0.03] {
                    let p = synthetic(kind, dt, period, 1., (0., 0.), 0., 2.);
                    let found = hits(&p, &settings(timeout));
                    let valid = kind < 2 && (period < 1. || timeout > 2.);
                    assert_eq!(!found.is_empty(), valid, "{timeout} {kind} {period} {dt}");
                    if valid {
                        let expected = if period < 1. {
                            if kind == 0 {
                                if dt == 0.01 {
                                    0.37
                                } else if dt == 0.016 {
                                    0.368
                                } else {
                                    0.39
                                }
                            } else if dt == 0.01 || dt == 0.03 {
                                0.36
                            } else {
                                0.368
                            }
                        } else if kind == 0 {
                            if dt == 0.01 {
                                1.75
                            } else if dt == 0.016 {
                                1.76
                            } else {
                                1.77
                            }
                        } else if dt == 0.01 {
                            1.72
                        } else if dt == 0.016 {
                            1.728
                        } else {
                            1.74
                        };
                        assert!((found[0] - expected).abs() < 1e-8);
                    }
                }
            }
        }
    }
}

#[test] // Kills direction, winding-threshold and broad shape regressions under shifted input.
fn tuning_daily_motion_and_near_threshold_arcs_do_not_fire() {
    for timeout in [1., 2.5] {
        for dt in [0.01, 0.016, 0.03] {
            for period in [0.25, 0.60] {
                for scale in [0.7, 1.3] {
                    for shift in [(0., 0.), (-2300., 900.)] {
                        for phase in [0., 0.41] {
                            for kind in 4..12 {
                                let turns = if kind == 11 { 0.8 } else { 2. };
                                assert!(
                                    hits(
                                        &synthetic(kind, dt, period, scale, shift, phase, turns),
                                        &settings(timeout)
                                    )
                                    .is_empty()
                                );
                            }
                            for turns in [1.30, 1.40, 1.45] {
                                assert!(
                                    hits(
                                        &synthetic(0, dt, period, scale, shift, phase, turns),
                                        &settings(timeout)
                                    )
                                    .is_empty(),
                                    "{timeout} {dt} {period} {scale} {phase} {turns}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
#[ignore = "Set ORBIT_GESTURE_LOG externally; private diagnostic points never enter fixtures"]
// Kills lost private-log events, slower recognition, and missing >=.1s gains on events 2..4.
fn tuning_private_poll_log() {
    let path = std::env::var_os("ORBIT_GESTURE_LOG").expect("ORBIT_GESTURE_LOG");
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let samples = raw["samples"].as_array().unwrap();
    let recorded = samples
        .iter()
        .filter(|v| v["recognized"] == true)
        .map(|v| v["t"].as_f64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(recorded.len(), 5);
    let p = samples
        .iter()
        .filter(|v| v["state"] == "tracking")
        .map(|v| {
            (
                v["point"][0].as_f64().unwrap(),
                v["point"][1].as_f64().unwrap(),
                v["t"].as_f64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    for timeout in [1., 2.5] {
        let found = hits(&p, &settings(timeout));
        assert_eq!(found.len(), 5);
        for i in 0..5 {
            assert!(found[i] <= recorded[i]);
            if (1..=3).contains(&i) {
                assert!(recorded[i] - found[i] >= 0.1);
            }
        }
        println!("poll_log timeout={timeout} hits={found:?}");
    }
}

// Frozen A control: the pre-change feed rules, sharing only the unchanged assess function.
fn legacy_feed(d: &mut Detector, x: f64, y: f64, now: f64, s: &Settings) -> bool {
    if now < d.blocked_until {
        return false;
    }
    if d.points.back().is_some_and(|p| now - p.2 > 0.45) {
        d.reset();
    }
    while d.points.front().is_some_and(|p| now - p.2 > s.timeout) {
        d.points.pop_front();
    }
    if d.points
        .back()
        .is_some_and(|p| (x - p.0).hypot(y - p.1) < 2.)
    {
        return false;
    }
    if d.points.len() == 512 {
        d.points.pop_front();
    }
    d.points.push_back((x, y, now));
    if d.points.len() < 24 {
        return false;
    }
    let p: Vec<_> = d.points.iter().copied().collect();
    for start in (0..p.len() - 23).step_by(6) {
        if assess(&p[start..], s) {
            d.reset();
            d.blocked_until = now + 1.5;
            return true;
        }
    }
    false
}
fn raw_input(env: &str) -> (f64, Vec<[f64; 4]>) {
    let path = std::env::var_os(env).expect(env);
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let rows = v["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| std::array::from_fn(|i| r[i].as_f64().unwrap()))
        .collect();
    (v["seconds"].as_f64().unwrap(), rows)
}
fn poll_raw(duration: f64, p: &[[f64; 4]], s: &Settings, legacy: bool) -> Vec<f64> {
    let mut d = Detector::default();
    let mut index = 0;
    let mut t = 0.;
    let mut interval = 0.08;
    let mut last = (p[0][1], p[0][2]);
    let mut moved = 0.;
    let mut result = vec![];
    while t <= duration {
        while index + 1 < p.len() && p[index + 1][0] <= t {
            index += 1;
        }
        let row = p[index];
        let position = (row[1], row[2]);
        if position == last {
            if t - moved > 0.5 {
                interval = 0.08;
            }
        } else {
            last = position;
            moved = t;
            interval = 0.03125;
            if row[3] == 1. {
                d.reset();
            } else if if legacy {
                legacy_feed(&mut d, row[1], row[2], t, s)
            } else {
                d.feed(row[1], row[2], t, s)
            } {
                result.push(t);
            }
        }
        t += interval;
    }
    result
}

#[test]
#[ignore = "Set ORBIT_GESTURE_RAW_CIRCLES externally; no private fixture is embedded"]
// Kills missing strokes, latency regression, or firing in idle gaps on the high-density record.
fn tuning_private_raw_circles() {
    let (duration, p) = raw_input("ORBIT_GESTURE_RAW_CIRCLES");
    let mut strokes = vec![];
    let mut start = p[0][0];
    for pair in p.windows(2) {
        if pair[1][0] - pair[0][0] > 0.3 {
            strokes.push((start, pair[0][0]));
            start = pair[1][0];
        }
    }
    strokes.push((start, p.last().unwrap()[0]));
    assert_eq!(strokes.iter().filter(|(a, b)| b - a > 0.2).count(), 19);
    for timeout in [1., 2.5] {
        let a = poll_raw(duration, &p, &settings(timeout), true);
        let b = poll_raw(duration, &p, &settings(timeout), false);
        println!(
            "raw_circles timeout={timeout} A={} B={} hits={b:?}",
            a.len(),
            b.len()
        );
        assert_eq!(a.len(), 12);
        assert_eq!(b.len(), 15);
        assert!(
            b.iter()
                .all(|t| strokes.iter().any(|(lo, hi)| t >= lo && t <= hi))
        );
        for (lo, hi) in &strokes {
            if let Some(old) = a.iter().find(|t| *t >= lo && *t <= hi) {
                assert!(b.iter().any(|t| t >= lo && t <= old));
            }
        }
    }
}

#[test]
#[ignore = "Set ORBIT_GESTURE_RAW_WORK externally; no private fixture is embedded"]
// Kills newly introduced everyday false positives and accidental loss of the A comparison.
fn tuning_private_raw_work() {
    let (duration, p) = raw_input("ORBIT_GESTURE_RAW_WORK");
    for timeout in [1., 2.5] {
        for min_radius in [100., 150., 200., 300.] {
            let s = Settings {
                min_radius,
                ..settings(timeout)
            };
            let a = poll_raw(duration, &p, &s, true);
            let b = poll_raw(duration, &p, &s, false);
            println!(
                "raw_work timeout={timeout} min={min_radius} A={} B={}",
                a.len(),
                b.len()
            );
            if timeout == 1. {
                assert!(a.is_empty() && b.is_empty());
            } else {
                assert!(b.len() <= a.len());
            }
        }
    }
}

#[test]
#[ignore = "Set ORBIT_GESTURE_SYNTH_DIR to the stage1 aggregate CSV directory"]
// Checks every historical row against the production detector without implementing C.
fn tuning_stage1_csv_rows() {
    fn fields(line: &str) -> Vec<String> {
        let mut quoted = false;
        let mut result = vec![String::new()];
        for c in line.chars() {
            if c == '"' {
                quoted = !quoted;
            } else if c == ',' && !quoted {
                result.push(String::new());
            } else {
                result.last_mut().unwrap().push(c);
            }
        }
        result
    }
    let directory = std::path::PathBuf::from(
        std::env::var_os("ORBIT_GESTURE_SYNTH_DIR").expect("ORBIT_GESTURE_SYNTH_DIR"),
    );
    let positive = std::fs::read_to_string(directory.join("synthetic-positive.csv")).unwrap();
    let names = [
        "circle",
        "ellipse",
        "small",
        "large",
        "line",
        "zigzag",
        "selection",
        "drag",
        "scrollbar",
        "jitter",
        "cw",
        "short_arc",
    ];
    let mut positive_rows = 0;
    let mut recognized = 0;
    for line in positive.lines().skip(1) {
        let f = fields(line);
        let kind = names.iter().position(|name| *name == f[1]).unwrap();
        let timeout = f[0].parse().unwrap();
        let period = f[2].parse().unwrap();
        let dt = f[3].parse::<f64>().unwrap() / 1000.;
        let found = hits(
            &synthetic(kind, dt, period, 1., (0., 0.), 0., 2.),
            &settings(timeout),
        );
        if !f[5].is_empty() {
            assert!(!found.is_empty());
            assert!(found[0] <= f[5].parse::<f64>().unwrap() + 1e-9);
        } else if f[4] != "A24s6" {
            assert!(found.is_empty());
        }
        recognized += usize::from(!found.is_empty());
        positive_rows += 1;
    }
    let negative = std::fs::read_to_string(directory.join("synthetic-negative.csv")).unwrap();
    let mut negative_rows = 0;
    for line in negative.lines().skip(1) {
        let f = fields(line);
        let kind = names.iter().position(|name| *name == f[0]).unwrap();
        let timeout = f[1].parse().unwrap();
        let dt = f[2].parse::<f64>().unwrap() / 1000.;
        let period = f[3].parse().unwrap();
        let scale = f[4].parse().unwrap();
        let shift = f[5]
            .trim_matches(['(', ')'])
            .split(',')
            .map(|s| s.trim().parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        let phase = f[6].parse().unwrap();
        assert_eq!(f[8], "0");
        assert!(
            hits(
                &synthetic(
                    kind,
                    dt,
                    period,
                    scale,
                    (shift[0], shift[1]),
                    phase,
                    if kind == 11 { 0.8 } else { 2. }
                ),
                &settings(timeout)
            )
            .is_empty()
        );
        negative_rows += 1;
    }
    assert_eq!((positive_rows, negative_rows), (144, 2304));
    assert!(recognized > 0);
    println!(
        "stage1_csv positive_rows={positive_rows} recognized_rows={recognized} negative_rows={negative_rows} false_positives=0"
    );
}

fn junk_then(curve_points: Vec<(f64, f64, f64)>) -> Vec<(f64, f64, f64)> {
    let mut p: Vec<_> = (0..6)
        .map(|i| (5000. + i as f64 * 10., 5000., i as f64 * 0.01))
        .collect();
    p.extend(curve_points.into_iter().map(|(x, y, t)| (x, y, t + 0.06)));
    p
}

#[test] // Kills an end bound of -6: a 7-point suffix at start 6 must never be assessed.
fn tuning_seven_point_suffix_is_not_a_candidate() {
    let s = settings(1.);
    let mut p = junk_then(curve(7, 1.5, 300., 300.));
    assert!(assess(&p[6..], &s));
    assert!(!assess(&p, &s));
    let last = p.pop().unwrap();
    assert!(!seed(p).feed(last.0, last.1, last.2, &s));
}

#[test] // Kills stride 5 and 7: only the suffix starting at 6 holds a full 1.5 turns.
fn tuning_stride_is_exactly_six() {
    let s = settings(1.);
    let mut p = junk_then(curve(9, 1.5, 300., 300.));
    assert!(assess(&p[6..], &s));
    assert!(!assess(&p[5..], &s)); // stride 5 starts here: one junk point breaks the bounds.
    assert!(!assess(&p[7..], &s)); // stride 7 starts here: 1.3125 turns only.
    let last = p.pop().unwrap();
    assert!(seed(p).feed(last.0, last.1, last.2, &s));
}

#[test] // Kills a looser purity threshold: 0.71 is rejected, 0.80 is accepted.
fn tuning_purity_threshold_near_boundary() {
    let s = settings(1.);
    let back_and_forth = |forward: usize, back: usize| {
        (0..=forward + back)
            .map(|i| {
                let steps = if i <= forward { i } else { 2 * forward - i };
                let a = TAU * steps as f64 / 48.;
                (300. * a.cos(), -300. * a.sin(), i as f64 * 0.01)
            })
            .collect::<Vec<_>>()
    };
    assert!(!assess(&back_and_forth(87, 15), &s)); // net 1.5 turns, purity 72/102.
    assert!(assess(&back_and_forth(81, 9), &s)); // net 1.5 turns, purity 72/90.
}

#[test] // Kills a looser deviation threshold: 0.70 is rejected, 0.60 is accepted.
fn tuning_deviation_threshold_near_boundary() {
    let s = Settings {
        min_radius: 5.,
        ..settings(1.)
    };
    let alternating = |inner: f64, outer: f64| {
        (0..97)
            .map(|i| {
                let r = if i % 2 == 0 { inner } else { outer };
                let a = TAU * 2. * i as f64 / 96.;
                (r * a.cos(), -r * a.sin(), i as f64 * 0.01)
            })
            .collect::<Vec<_>>()
    };
    assert!(!assess(&alternating(60., 340.), &s));
    assert!(assess(&alternating(80., 320.), &s));
}
