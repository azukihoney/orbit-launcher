//! UI-thread notifications live outside App's RefCell. Never spin by reposting.
use std::{cell::Cell, rc::Rc};
use windows_sys::Win32::Foundation::{HWND, RECT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DialogToken {
    pub hwnd: HWND,
    pub generation: u64,
}
#[derive(Clone, Copy)]
pub struct Dpi {
    pub value: u32,
    pub rect: RECT,
    pub reveal: Option<RevealPlan>,
    pub moving: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DpiAttempt {
    pub device: [u16; 32],
    pub target: u32,
}
#[derive(Clone, Copy)]
pub struct RevealPlan {
    pub generation: u64,
    pub cursor: windows_sys::Win32::Foundation::POINT,
}
#[derive(Clone, Copy)]
pub enum Scope {
    Modal,
    Menu,
    Moving,
}

#[derive(Default)]
pub struct Runtime {
    modal: Cell<u32>,
    menu: Cell<u32>,
    moving: Cell<u32>,
    pub dialog: Cell<Option<DialogToken>>,
    pub close_dialog: Cell<Option<DialogToken>>,
    pub destroyed_dialog: Cell<Option<DialogToken>>,
    pub close_main: Cell<bool>,
    pub dpi: Cell<Option<Dpi>>,
    pub dpi_check: Cell<bool>,
    pub dpi_attempted: Cell<Option<DpiAttempt>>,
    pub recycle: Cell<Option<u64>>,
    pub recycle_done: Cell<Option<(u64, i32)>>,
    pub taskbar_message: Cell<u32>,
    pub restore_tray: Cell<bool>,
    pub reveal_plan: Cell<Option<RevealPlan>>,
    pub outside_drop: Cell<bool>,
    pub outside_cancel: Cell<bool>,
}
impl Runtime {
    pub fn enter_reveal(self: &Rc<Self>, plan: RevealPlan) -> RevealGuard {
        let previous = self.reveal_plan.replace(Some(plan));
        RevealGuard {
            state: self.clone(),
            previous,
        }
    }
    fn depth(&self, kind: Scope) -> &Cell<u32> {
        match kind {
            Scope::Modal => &self.modal,
            Scope::Menu => &self.menu,
            Scope::Moving => &self.moving,
        }
    }
    pub fn enter(self: &Rc<Self>, kind: Scope) -> Guard {
        // A whole nested operation can begin and end between two mouse polls.
        self.outside_cancel.set(true);
        let depth = self.depth(kind);
        depth.set(depth.get() + 1);
        Guard {
            state: self.clone(),
            kind,
        }
    }
    pub fn nested(&self) -> bool {
        self.modal.get() != 0 || self.menu.get() != 0 || self.moving.get() != 0
    }
    pub fn moving(&self) -> bool {
        self.moving.get() != 0
    }
    pub fn editing_blocked(&self) -> bool {
        self.dialog.get().is_some() || self.nested()
    }
    pub fn quit_blocked(&self) -> bool {
        self.editing_blocked() || self.recycle.get().is_some()
    }
    pub fn record_recycle_done(&self, generation: u64, result: i32) {
        if self.recycle.get() == Some(generation) && self.recycle_done.get().is_none() {
            self.recycle_done.set(Some((generation, result)));
        }
    }
}
pub struct RevealGuard {
    state: Rc<Runtime>,
    previous: Option<RevealPlan>,
}
impl Drop for RevealGuard {
    fn drop(&mut self) {
        self.state.reveal_plan.set(self.previous);
    }
}
pub struct Guard {
    state: Rc<Runtime>,
    kind: Scope,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let depth = self.state.depth(self.kind);
        depth.set(depth.get().checked_sub(1).expect("scope depth is balanced"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scopes_restore_on_early_return_and_unwind_without_clearing_other_operations() {
        let r = Rc::new(Runtime::default());
        r.recycle.set(Some(7));
        fn early_return(r: &Rc<Runtime>) -> Result<(), ()> {
            let _a = r.enter(Scope::Menu);
            let _b = r.enter(Scope::Modal);
            let failure: Result<(), ()> = Err(());
            failure?;
            Ok(())
        }
        let result = early_return(&r);
        assert!(result.is_err());
        assert!(!r.nested());
        assert!(r.quit_blocked());
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _a = r.enter(Scope::Moving);
            panic!("test scope unwind");
        }));
        assert!(!r.nested());
        assert_eq!(r.recycle.get(), Some(7));
    }
    #[test]
    fn latest_dpi_and_completion_survive_nested_scopes() {
        let r = Rc::new(Runtime::default());
        {
            let _g = r.enter(Scope::Menu);
            r.dpi.set(Some(Dpi {
                value: 120,
                rect: RECT::default(),
                reveal: None,
                moving: false,
            }));
            r.dpi.set(Some(Dpi {
                value: 144,
                rect: RECT::default(),
                reveal: None,
                moving: false,
            }));
            r.recycle_done.set(Some((4, 0)));
        }
        assert_eq!(r.dpi.take().unwrap().value, 144);
        assert_eq!(r.recycle_done.take(), Some((4, 0)));
        assert!(r.dpi.take().is_none());
    }
    #[test]
    fn stale_or_duplicate_completions_cannot_replace_the_active_result() {
        let r = Runtime::default();
        r.recycle.set(Some(9));
        r.record_recycle_done(8, -1);
        assert!(r.recycle_done.get().is_none());
        r.record_recycle_done(9, 0);
        r.record_recycle_done(8, -1);
        r.record_recycle_done(9, -2);
        assert_eq!(r.recycle_done.take(), Some((9, 0)));
        r.recycle.set(None);
        r.record_recycle_done(9, -1);
        assert!(r.recycle_done.get().is_none());
    }
}
