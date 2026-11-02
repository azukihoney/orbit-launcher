//! Button-independent sampled outside-click decisions. No OS calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub previous: u8,
    pub outside_press: u8,
    pub inside_seen: u8,
    pub dropped: u8,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub buttons: u8,
    pub inside: bool,
    pub tray: bool,
    pub pinned: bool,
    pub blocked: bool,
    pub drop_received: bool,
}

pub fn step(state: State, input: Input) -> (State, bool) {
    let buttons = input.buttons & 31;
    if input.pinned || input.blocked {
        return (
            State {
                previous: buttons,
                ..State::default()
            },
            false,
        );
    }
    let inside = input.inside || input.tray;
    let active = state.previous | buttons;
    let inside_seen = state.inside_seen | if inside { active } else { 0 };
    let dropped = state.dropped | if input.drop_received { active } else { 0 };
    let released = state.previous & !buttons;
    let close = released & state.outside_press & !inside_seen & !dropped != 0;
    let pressed = buttons & !state.previous;
    let outside_press = state.outside_press | if inside { 0 } else { pressed };
    (
        State {
            previous: buttons,
            outside_press: outside_press & buttons,
            inside_seen: inside_seen & buttons,
            dropped: dropped & buttons,
        },
        close,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_truth_table() {
        // Reject omitted buttons, down-edge closure, ignored guards, lost history and cross-button contamination.
        for button in [1, 2, 4, 8, 16] {
            for bits in 0..1024u16 {
                let b = |n: u32| bits & (1u16 << n) != 0u16;
                let mask = |n| if b(n) { button } else { 0 };
                let state = State {
                    previous: mask(0),
                    outside_press: mask(2),
                    inside_seen: mask(3),
                    dropped: mask(4),
                };
                let input = Input {
                    buttons: mask(1),
                    inside: b(5),
                    tray: b(6),
                    pinned: b(7),
                    blocked: b(8),
                    drop_received: b(9),
                };
                let (next, close) = step(state, input);
                let allowed = !b(7) && !b(8);
                let expected =
                    allowed && b(0) && !b(1) && b(2) && !b(3) && !b(4) && !b(5) && !b(6) && !b(9);
                assert_eq!(close, expected, "button={button} bits={bits}");
                assert_eq!(next.previous, mask(1), "previous: {bits}");
                let newly_pressed = !b(0) && b(1);
                let active = b(1);
                let seen = (b(3) || b(5) || b(6)) && active && allowed;
                let dropped = (b(4) || b(9)) && active && allowed;
                let outside = (b(2) || newly_pressed && !b(5) && !b(6)) && active && allowed;
                assert_eq!(
                    next.inside_seen,
                    if seen { button } else { 0 },
                    "inside: {bits}"
                );
                assert_eq!(
                    next.dropped,
                    if dropped { button } else { 0 },
                    "drop: {bits}"
                );
                assert_eq!(
                    next.outside_press,
                    if outside { button } else { 0 },
                    "press: {bits}"
                );
            }
        }
    }

    #[test]
    fn inside_history_and_drop_are_independent() {
        // Reject either redundant-looking guard being removed: each has a case without the other.
        let pressed = Input {
            buttons: 1,
            ..Input::default()
        };
        let (s, _) = step(State::default(), pressed);
        let (inside, _) = step(
            s,
            Input {
                inside: true,
                ..pressed
            },
        );
        assert!(!step(inside, Input::default()).1);
        let (drop, _) = step(
            s,
            Input {
                drop_received: true,
                ..pressed
            },
        );
        assert!(!step(drop, Input::default()).1);
        assert!(step(s, Input::default()).1);
    }

    #[test]
    fn held_on_reveal_and_blocked_gesture_do_not_close() {
        // Reject treating an already-held button or a press spanning a blocked scope as a new click.
        let held = State {
            previous: 31,
            ..State::default()
        };
        assert!(!step(held, Input::default()).1);
        let (s, _) = step(
            State::default(),
            Input {
                buttons: 31,
                ..Input::default()
            },
        );
        let (s, _) = step(
            s,
            Input {
                buttons: 31,
                blocked: true,
                ..Input::default()
            },
        );
        assert!(!step(s, Input::default()).1);
        assert!(step(held, Input::default()).0 == State::default());
        let (s, _) = step(
            State::default(),
            Input {
                buttons: 31,
                ..Input::default()
            },
        );
        assert!(step(s, Input::default()).1);
    }

    #[test]
    fn simultaneous_and_subsequent_clicks() {
        // Reject collapsing all buttons to a single any-down bit or retaining suppression across clicks.
        let (s, _) = step(
            State::default(),
            Input {
                buttons: 3,
                ..Input::default()
            },
        );
        assert!(
            step(
                s,
                Input {
                    buttons: 2,
                    ..Input::default()
                }
            )
            .1
        );
        let (s, _) = step(
            State::default(),
            Input {
                buttons: 1,
                inside: true,
                ..Input::default()
            },
        );
        let (s, close) = step(s, Input::default());
        assert!(!close);
        let (s, _) = step(
            s,
            Input {
                buttons: 1,
                ..Input::default()
            },
        );
        assert!(step(s, Input::default()).1);
    }
}
