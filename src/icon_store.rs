//! One coalescing worker. HICON ownership crosses threads as an owned integer handle.
use crate::{model::Entry, os, paint::Icon};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Key {
    kind: String,
    target: String,
    pub pixels: u32,
}
impl Key {
    pub fn new(entry: &Entry, pixels: u32) -> Self {
        Self {
            kind: entry.kind.clone(),
            target: entry.target.clone(),
            pixels,
        }
    }
}
// Zero pixels means the unchanged legacy acquisition path; large mode always asks >0.
struct OwnedHandle(usize);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if self.0 != 0 {
            // Owned HICON, never a shared Shell image-list handle.
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon(self.0 as _);
            }
        }
    }
}
struct Request {
    entry: Entry,
    key: Key,
    generation: u64,
}
struct ResultIcon {
    key: Key,
    generation: u64,
    icon: OwnedHandle,
}
#[derive(Default)]
struct Work {
    queue: VecDeque<Request>,
    active: Option<(Key, u64)>,
    closed: bool,
}

pub struct Store {
    icons: HashMap<Key, Icon>,
    wanted: HashSet<Key>,
    pending: HashMap<Key, u64>,
    generation: u64,
    mode: u32,
    work: Arc<(Mutex<Work>, Condvar)>,
    rx: mpsc::Receiver<ResultIcon>,
}
fn eligible(wanted: &HashSet<Key>, generation: u64, key: &Key, result_generation: u64) -> bool {
    wanted.contains(key) && generation == result_generation
}
impl Store {
    pub fn new() -> Self {
        let work = Arc::new((Mutex::new(Work::default()), Condvar::new()));
        let (tx, rx) = mpsc::channel();
        let worker = Arc::clone(&work);
        thread::spawn(move || {
            // Keep GDI+ alive until any in-flight resize finishes, even if UI exits.
            let mut large_graphics = None;
            loop {
                let request = {
                    let (lock, wake) = &*worker;
                    let mut work = lock.lock().unwrap();
                    while work.queue.is_empty() && !work.closed {
                        work = wake.wait(work).unwrap();
                    }
                    if work.closed {
                        return;
                    }
                    let request = work.queue.pop_front().unwrap();
                    work.active = Some((request.key.clone(), request.generation));
                    request
                };
                if request.key.pixels > 0 && large_graphics.is_none() {
                    large_graphics = crate::paint::GdiPlus::new().ok();
                }
                let icon = if request.key.pixels == 0 {
                    os::file_icon(&request.entry)
                } else {
                    os::large_file_icon(&request.entry, request.key.pixels)
                };
                #[cfg(feature = "review-test")]
                crate::icon_only_probe::event(
                    "icon_acquired",
                    serde_json::json!({"pixels":request.key.pixels,"generation":request.generation,"valid":!icon.0.is_null()}),
                );
                let owned = OwnedHandle(icon.0 as usize);
                std::mem::forget(icon);
                let result = ResultIcon {
                    key: request.key,
                    generation: request.generation,
                    icon: owned,
                };
                if tx.send(result).is_err() {
                    return;
                }
                worker.0.lock().unwrap().active = None;
            }
        });
        Self {
            icons: HashMap::new(),
            wanted: HashSet::new(),
            pending: HashMap::new(),
            generation: 0,
            mode: 0,
            work,
            rx,
        }
    }
    pub fn sync(&mut self, entries: Vec<(Entry, u32)>, mode: u32) {
        if self.mode != mode {
            self.generation = self.generation.wrapping_add(1);
            self.mode = mode;
        }
        self.wanted = entries
            .iter()
            .map(|(entry, px)| Key::new(entry, *px))
            .collect();
        self.icons.retain(|key, _| self.wanted.contains(key));
        let (lock, wake) = &*self.work;
        let mut work = lock.lock().unwrap();
        work.queue.clear();
        self.pending.clear();
        for (entry, pixels) in entries {
            let key = Key::new(&entry, pixels);
            if self.icons.contains_key(&key) || self.pending.contains_key(&key) {
                continue;
            }
            self.pending.insert(key.clone(), self.generation);
            if work.active.as_ref() == Some(&(key.clone(), self.generation)) {
                continue;
            }
            work.queue.push_back(Request {
                entry,
                key,
                generation: self.generation,
            });
        }
        wake.notify_one();
    }
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(mut result) = self.rx.try_recv() {
            if self.pending.get(&result.key) == Some(&result.generation) {
                self.pending.remove(&result.key);
            }
            if eligible(
                &self.wanted,
                self.generation,
                &result.key,
                result.generation,
            ) {
                let raw = std::mem::take(&mut result.icon.0);
                self.icons.insert(result.key, Icon(raw as _));
                changed = true;
            }
        }
        changed
    }
    pub fn get(&self, entry: &Entry, pixels: u32) -> Option<&Icon> {
        self.icons.get(&Key::new(entry, pixels))
    }
    pub fn ready(&self) -> bool {
        self.pending.is_empty()
    }
    #[cfg(feature = "review-test")]
    pub fn counts(&self) -> serde_json::Value {
        serde_json::json!({"large":self.icons.keys().filter(|key|key.pixels>0).count(),
            "total":self.icons.len(),"pending":self.pending.len(),"generation":self.generation})
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let mut work = self.work.0.lock().unwrap();
        work.closed = true;
        work.queue.clear();
        self.work.1.notify_one();
        // No join: a stalled Shell extension must not hold UI shutdown hostage.
        // Dropping rx destroys queued OwnedHandles; failed worker sends also drop them.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn isolated() -> (mpsc::Sender<ResultIcon>, Store) {
        let (tx, rx) = mpsc::channel();
        (
            tx,
            Store {
                icons: HashMap::new(),
                wanted: HashSet::new(),
                pending: HashMap::new(),
                generation: 2,
                mode: 48,
                work: Arc::new((Mutex::new(Work::default()), Condvar::new())),
                rx,
            },
        )
    }
    #[test]
    #[ignore = "process resource counts require an isolated --test-threads=1 run"]
    fn real_worker_cycles_release_large_icons_and_gdi_objects() {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetGuiResources};
        let entry = Entry::new(
            "sample",
            &format!(r"{}\System32\cmd.exe", std::env::var("SystemRoot").unwrap()),
            "launch",
        );
        let mut store = Store::new();
        let ready = |store: &mut Store| {
            let start = std::time::Instant::now();
            loop {
                store.poll();
                if store.ready() {
                    break;
                }
                assert!(start.elapsed().as_secs() < 20, "icon worker timed out");
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        };
        let resources = || unsafe {
            (
                GetGuiResources(GetCurrentProcess(), 0),
                GetGuiResources(GetCurrentProcess(), 1),
            )
        };
        store.sync(vec![(entry.clone(), 48), (entry.clone(), 0)], 48);
        ready(&mut store);
        let before = resources();
        for px in [48, 60, 72].into_iter().cycle().take(30) {
            store.sync(vec![(entry.clone(), px), (entry.clone(), 0)], px);
            ready(&mut store);
            assert!(!store.get(&entry, px).unwrap().0.is_null());
            assert_eq!(store.icons.len(), 2);
            store.sync(vec![(entry.clone(), 0)], 0);
            ready(&mut store);
            assert!(store.icons.keys().all(|key| key.pixels == 0));
            assert_eq!(store.icons.len(), 1);
        }
        let after = resources();
        assert!(
            after.0 <= before.0 + 4 && after.1 <= before.1 + 4,
            "resource growth: {before:?} -> {after:?}"
        );
        println!("resource counts: GDI/USER {before:?} -> {after:?}; large while off=0; 30 cycles");
    }
    #[test]
    fn delayed_results_preserve_new_pending_and_removed_keys_stay_removed() {
        let (tx, mut store) = isolated();
        let entry = Entry::new("fake", "synthetic.bat", "launch");
        let key = Key::new(&entry, 48);
        store.wanted.insert(key.clone());
        store.pending.insert(key.clone(), 2);
        tx.send(ResultIcon {
            key: key.clone(),
            generation: 1,
            icon: OwnedHandle(0),
        })
        .unwrap_or_else(|_| panic!("closed receiver"));
        assert!(!store.poll());
        assert_eq!(store.pending.get(&key), Some(&2));
        assert!(store.icons.is_empty());
        store.wanted.clear();
        tx.send(ResultIcon {
            key: key.clone(),
            generation: 2,
            icon: OwnedHandle(0),
        })
        .unwrap_or_else(|_| panic!("closed receiver"));
        assert!(!store.poll());
        assert!(store.icons.is_empty());
        assert!(store.ready());
        store.wanted.insert(key.clone());
        tx.send(ResultIcon {
            key: key.clone(),
            generation: 2,
            icon: OwnedHandle(0),
        })
        .unwrap_or_else(|_| panic!("closed receiver"));
        assert!(store.poll());
        assert!(store.get(&entry, 48).is_some());
    }
    #[test]
    fn queue_coalesces_and_off_releases_large_entries() {
        let (_, mut store) = isolated();
        let entry = Entry::new("fake", "synthetic.bat", "launch");
        for px in [48, 60, 72, 48] {
            store.sync(
                vec![(entry.clone(), px), (entry.clone(), px), (entry.clone(), 0)],
                px,
            );
            assert_eq!(store.work.0.lock().unwrap().queue.len(), 2);
        }
        store
            .icons
            .insert(Key::new(&entry, 48), Icon(std::ptr::null_mut()));
        store
            .icons
            .insert(Key::new(&entry, 0), Icon(std::ptr::null_mut()));
        store.sync(vec![(entry.clone(), 0)], 0);
        assert_eq!(store.icons.len(), 1);
        assert!(store.icons.keys().all(|key| key.pixels == 0));
        assert!(store.work.0.lock().unwrap().queue.is_empty());
    }
    #[test]
    fn wanted_and_generation_have_independent_jobs() {
        let key = Key::new(&Entry::new("test", "synthetic.bat", "launch"), 48);
        let wanted = HashSet::from([key.clone()]);
        assert!(eligible(&wanted, 2, &key, 2));
        assert!(!eligible(&wanted, 2, &key, 1)); // Same wanted key, old generation.
        assert!(!eligible(&HashSet::new(), 2, &key, 2)); // Same generation, removed key.
        assert_ne!(
            key,
            Key {
                pixels: 60,
                ..key.clone()
            }
        );
        assert_ne!(
            key,
            Key {
                pixels: 0,
                ..key.clone()
            }
        );
    }
}
