//! Two-stage Ctrl-C for `sdc convert` (spec §8 "Ctrl-C", Task 44).
//!
//! - First Ctrl-C: cooperative cancel. Running files stop at their next band boundary, no new file starts,
//!   `batch_done` lists the cancelled files and the batch exits 1.
//! - Second Ctrl-C: exit 3 at once, without waiting for the running bands (a native encode call cannot be
//!   interrupted, so a long band could otherwise keep the process alive).
//!
//! Outputs are written with `write_atomic` (temp file in the target directory, then rename), so an abort never
//! leaves a partial `.dds`. It can leave `.sdc-*.tmp` files behind in the output folders, and a file that was
//! renamed into place just before the abort may have no manifest record yet. Both are harmless: the next run
//! reports the record-less output as `OUT-UNKNOWN-EXISTING` and `--on-existing overwrite` replaces it; the temp
//! files can be deleted by hand.
use engine::api::CancelToken;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Exit code of the second Ctrl-C (spec §8: fatal).
pub const ABORT_EXIT_CODE: i32 = 3;

/// One interrupt. `seen` counts the interrupts so far (injected so the logic is testable without a console);
/// `abort` is called with [`ABORT_EXIT_CODE`] from the second interrupt on.
pub fn on_interrupt(seen: &AtomicU32, cancel: &CancelToken, abort: impl Fn(i32)) {
    if seen.fetch_add(1, Ordering::SeqCst) == 0 {
        eprintln!("cancelling: finishing current bands... (Ctrl-C again to exit immediately)");
        cancel.cancel();
    } else {
        eprintln!(
            "aborting: exit 3; unfinished temp files (.sdc-*.tmp) may remain in the output folders"
        );
        abort(ABORT_EXIT_CODE);
    }
}

/// Install the process-wide handler. Call it before planning, so a Ctrl-C during a long scan is not lost.
pub fn install(cancel: CancelToken) -> Result<(), ctrlc::Error> {
    let seen = Arc::new(AtomicU32::new(0));
    ctrlc::set_handler(move || on_interrupt(&seen, &cancel, |c| std::process::exit(c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn first_interrupt_cancels_second_aborts_with_3() {
        let (seen, cancel, exited) = (AtomicU32::new(0), CancelToken::default(), Cell::new(None));
        on_interrupt(&seen, &cancel, |c| exited.set(Some(c)));
        assert!(cancel.is_cancelled());
        assert_eq!(exited.get(), None, "the first Ctrl-C must not exit");
        on_interrupt(&seen, &cancel, |c| exited.set(Some(c)));
        assert_eq!(exited.get(), Some(3));
        on_interrupt(&seen, &cancel, |c| exited.set(Some(c)));
        assert_eq!(exited.get(), Some(3), "every later Ctrl-C keeps aborting");
    }
}
