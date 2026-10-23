//! Per-thread statics for Razdor, declared with [`local!`]: the same syntax and the same
//! `with` / `set` / `get` / `take` / `replace` / `with_borrow` calls as std's `thread_local!`,
//! but without a TLS key per static.
//!
//! std's `thread_local!` takes one pthread key for each static on Android (Rust's fallback
//! where the target has no native TLS), and bionic gives a process about 128 of them. Razdor has
//! more statics than that, so the army window aborted with "out of TLS keys" on a phone. Here
//! every [`Local`] takes an id on its first use, and all of them share ONE std thread local: a
//! per-thread table of boxed values, indexed by that id.
//!
//! Only this module may use std's `thread_local!` (a test checks it).

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};

thread_local! {
    /// This thread's values: slot `id` holds the value of the local with that id. The one std
    /// thread local of the crate, so one TLS key.
    static SLOTS: RefCell<Vec<Option<Box<dyn Any>>>> = const { RefCell::new(Vec::new()) };
}

/// The ids handed out so far (0 is "no id yet").
static NEXT_ID: AtomicUsize = AtomicUsize::new(1);

/// One per-thread value of type `T`, made by `init` on the thread's first use. Declare it with
/// [`local!`]; the value lives in the thread's table until the thread ends, and is dropped then.
pub struct Local<T: 'static> {
    /// The id in the table (0 until the first use).
    id: AtomicUsize,
    init: fn() -> T,
}

impl<T: 'static> Local<T> {
    /// A local whose value `init` makes (the macro passes a closure, which coerces to `fn`).
    pub const fn new(init: fn() -> T) -> Local<T> {
        Local { id: AtomicUsize::new(0), init }
    }

    /// This local's id in the tables: taken on the first use, the same on every thread.
    fn id(&self) -> usize {
        let id = self.id.load(Ordering::Acquire);
        if id != 0 {
            return id;
        }
        let fresh = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        match self.id.compare_exchange(0, fresh, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => fresh,
            Err(taken) => taken,
        }
    }

    /// The address of this thread's value, made first when it is not there yet. The box is
    /// never moved or freed before the thread ends, so the address stays valid.
    fn slot(&self) -> *const T {
        let id = self.id();
        let present = SLOTS.with(|slots| slots.borrow().get(id).is_some_and(Option::is_some));
        if !present {
            // Made outside the table's borrow: `init` may use other locals.
            let made: Box<dyn Any> = Box::new((self.init)());
            SLOTS.with(|slots| {
                let mut slots = slots.borrow_mut();
                if slots.len() <= id {
                    slots.resize_with(id + 1, || None);
                }
                if slots[id].is_none() {
                    slots[id] = Some(made);
                }
            });
        }
        SLOTS.with(|slots| {
            let slots = slots.borrow();
            let boxed = slots[id].as_ref().expect("the slot was made above");
            boxed.downcast_ref::<T>().expect("a slot holds its own local's type") as *const T
        })
    }

    /// Runs `f` on this thread's value. Nested calls on other locals (or this one) are fine:
    /// the table is not borrowed while `f` runs.
    pub fn with<R>(&'static self, f: impl FnOnce(&T) -> R) -> R {
        let value = self.slot();
        // SAFETY: the box behind `value` lives in this thread's table until the thread ends,
        // and nothing takes it out of the table: only `&T` is handed out, so the value is
        // never moved or dropped while `f` runs (its mutation goes through the cell types).
        f(unsafe { &*value })
    }
}

impl<T: Copy + 'static> Local<Cell<T>> {
    /// The value of the cell (`Cell::get`).
    pub fn get(&'static self) -> T {
        self.with(Cell::get)
    }
}

impl<T: 'static> Local<Cell<T>> {
    /// Stores `value` in the cell (`LocalKey::set`).
    pub fn set(&'static self, value: T) {
        self.with(|cell| cell.set(value))
    }

    /// Stores `value` and returns the old one (`LocalKey::replace`).
    pub fn replace(&'static self, value: T) -> T {
        self.with(|cell| cell.replace(value))
    }

    /// Takes the value out, leaving the default (`LocalKey::take`).
    pub fn take(&'static self) -> T
    where
        T: Default,
    {
        self.with(Cell::take)
    }
}

impl<T: 'static> Local<RefCell<T>> {
    /// Runs `f` on a shared borrow of the cell's value (`LocalKey::with_borrow`).
    pub fn with_borrow<R>(&'static self, f: impl FnOnce(&T) -> R) -> R {
        self.with(|cell| f(&cell.borrow()))
    }

    /// Runs `f` on a mutable borrow of the cell's value (`LocalKey::with_borrow_mut`).
    pub fn with_borrow_mut<R>(&'static self, f: impl FnOnce(&mut T) -> R) -> R {
        self.with(|cell| f(&mut cell.borrow_mut()))
    }

    /// Replaces the cell's value (`LocalKey::set`).
    pub fn set(&'static self, value: T) {
        self.with(|cell| *cell.borrow_mut() = value)
    }

    /// Replaces the cell's value and returns the old one (`LocalKey::replace`).
    pub fn replace(&'static self, value: T) -> T {
        self.with(|cell| cell.replace(value))
    }

    /// Takes the value out, leaving the default (`LocalKey::take`).
    pub fn take(&'static self) -> T
    where
        T: Default,
    {
        self.with(RefCell::take)
    }
}

/// Declares per-thread statics with the syntax of std's `thread_local!`:
///
/// ```ignore
/// local! {
///     /// Doc comments and attributes pass through.
///     pub static COUNT: Cell<u32> = const { Cell::new(0) };
///     static NAME: RefCell<String> = RefCell::new(String::new());
/// }
/// ```
///
/// Each static becomes a `static NAME: Local<T>`; use it as a `thread_local!` static.
#[macro_export]
macro_rules! local {
    () => {};
    ($(#[$attr:meta])* $vis:vis static $name:ident : $t:ty = const { $init:expr } ; $($rest:tt)*) => {
        $(#[$attr])* $vis static $name: $crate::local::Local<$t> = $crate::local::Local::new(|| $init);
        $crate::local!($($rest)*);
    };
    ($(#[$attr:meta])* $vis:vis static $name:ident : $t:ty = $init:expr ; $($rest:tt)*) => {
        $(#[$attr])* $vis static $name: $crate::local::Local<$t> = $crate::local::Local::new(|| $init);
        $crate::local!($($rest)*);
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    local! {
        static COUNTER: Cell<u32> = const { Cell::new(10) };
        static TEXT: RefCell<String> = RefCell::new(String::from("start"));
    }

    static INITS: AtomicUsize = AtomicUsize::new(0);
    local! {
        static ONCE: RefCell<u32> = { INITS.fetch_add(1, Ordering::SeqCst); RefCell::new(7) };
    }

    local! {
        static OUTER: Cell<u32> = const { Cell::new(1) };
        static INNER: Cell<u32> = const { Cell::new(100) };
    }

    /// Dropped when its thread ends (the table frees it then).
    struct Dropper;
    static DROPPED: AtomicBool = AtomicBool::new(false);
    impl Drop for Dropper {
        fn drop(&mut self) {
            DROPPED.store(true, Ordering::SeqCst);
        }
    }
    local! {
        static DROPS: Dropper = Dropper;
    }

    #[test]
    fn each_thread_has_its_own_value() {
        COUNTER.set(3);
        let other = std::thread::spawn(|| {
            let before = COUNTER.get();
            COUNTER.set(99);
            (before, COUNTER.get())
        })
        .join()
        .unwrap();
        assert_eq!(other, (10, 99), "a new thread starts from the initial value");
        assert_eq!(COUNTER.get(), 3, "the other thread did not change this one");
    }

    #[test]
    fn init_runs_once_per_thread_on_first_use() {
        let before = INITS.load(Ordering::SeqCst);
        ONCE.with_borrow(|v| assert_eq!(*v, 7));
        ONCE.with_borrow_mut(|v| *v += 1);
        ONCE.with_borrow(|v| assert_eq!(*v, 8));
        assert_eq!(INITS.load(Ordering::SeqCst) - before, 1, "the init ran once for this thread");
        std::thread::spawn(|| ONCE.with_borrow(|v| assert_eq!(*v, 7))).join().unwrap();
        assert_eq!(INITS.load(Ordering::SeqCst) - before, 2, "another thread makes its own");
    }

    #[test]
    fn nested_with_on_other_locals_works() {
        let sum = OUTER.with(|o| {
            let before = o.get();
            INNER.with(|i| {
                // Setting the outer local inside the inner call is fine too.
                OUTER.set(5);
                before + i.get()
            })
        });
        assert_eq!(sum, 101);
        assert_eq!(OUTER.get(), 5);
    }

    #[test]
    fn cell_and_refcell_helpers() {
        assert_eq!(COUNTER.replace(4), 10);
        assert_eq!(COUNTER.get(), 4);
        assert_eq!(COUNTER.take(), 4);
        assert_eq!(COUNTER.get(), 0);
        TEXT.set("abc".to_string());
        assert_eq!(TEXT.replace("xyz".to_string()), "abc");
        TEXT.with_borrow_mut(|s| s.push('!'));
        assert_eq!(TEXT.with_borrow(|s| s.clone()), "xyz!");
        assert_eq!(TEXT.take(), "xyz!");
        assert_eq!(TEXT.with_borrow(|s| s.len()), 0);
    }

    #[test]
    fn values_are_dropped_when_the_thread_ends() {
        assert!(!DROPPED.load(Ordering::SeqCst));
        std::thread::spawn(|| DROPS.with(|_| ())).join().unwrap();
        assert!(DROPPED.load(Ordering::SeqCst), "the thread's value was dropped with the thread");
    }

    /// Fails if std's `thread_local!` comes back anywhere in `src/` but here: each one takes a
    /// TLS key, and Android has about 128 of them for the whole process.
    #[test]
    fn no_std_thread_local_outside_this_module() {
        fn walk(dir: &std::path::Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("local.rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    if text.contains("thread_local!") {
                        found.push(path.display().to_string());
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut found);
        assert!(found.is_empty(), "use razdor's local! instead of thread_local! in: {found:?}");
    }
}
