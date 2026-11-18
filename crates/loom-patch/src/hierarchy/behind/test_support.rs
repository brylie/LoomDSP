//! Counts port follows in both the plain walks and the memoized walks.

use std::cell::Cell;

thread_local! {
    static REMAINING: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Restores the previous budget when the measured call returns or panics.
struct RestoreBudget(Option<usize>);

impl Drop for RestoreBudget {
    fn drop(&mut self) {
        REMAINING.set(self.0);
    }
}

/// Runs one pass with a follow budget on this thread and returns its result
/// and the number of follows. Stops the walk at the first excess step.
pub fn with_follow_budget<T>(limit: usize, run: impl FnOnce() -> T) -> (T, usize) {
    let _restore = RestoreBudget(REMAINING.replace(Some(limit)));
    let result = run();
    (result, limit - REMAINING.get().expect("a follow budget is set"))
}

/// Charges one step to the active budget, if the test set one.
pub(crate) fn follow() {
    if let Some(remaining) = REMAINING.get() {
        REMAINING.set(Some(remaining.checked_sub(1).expect("port follow budget exceeded")));
    }
}
