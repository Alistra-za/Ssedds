//! Two-stage Ctrl-C for `sdc convert` (spec §8 "Ctrl-C", Task 44).
//!
//! - First Ctrl-C: cooperative cancel. Running files stop at their next band boundary, no new file starts,
//!   `batch_done` lists the cancelled files and the batch exits 1.
//! - Second Ctrl-C: exit 3 at once, without waiting for the running bands (a native encode call cannot be
//!   interrupted, so a long band could otherwise keep the process alive) and without writing anything.
//!
//! The handler itself does no I/O (Codex-CLI 1, DECISIONS Note K); a Ctrl-C while planning is honoured too
//! (`Engine::plan` takes the token).
//!
//! Outputs are written with `write_atomic` (temp file in the target directory, then rename), so an abort never
//! leaves a partial `.dds`. It can leave `.sdc-*.tmp` files behind in the output folders, and a file that was
//! renamed into place just before the abort may have no manifest record yet. Both are harmless: the next run
//! reports the record-less output as `OUT-UNKNOWN-EXISTING` and `--on-existing overwrite` replaces it; the temp
//! files can be deleted by hand.
use engine::api::CancelToken;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};

/// Exit code of the second Ctrl-C (spec §8: fatal).
pub const ABORT_EXIT_CODE: i32 = 3;

/// The note printed (by a helper thread, never by the handler) after the first Ctrl-C.
const CANCELLING_NOTE: &str =
    "cancelling: finishing current bands... (Ctrl-C again to exit immediately)";

/// One interrupt. It does no I/O (Codex-CLI 1): a full, undrained stderr pipe would block a write here, and
/// ctrlc runs its callbacks one at a time, so a blocked first callback would keep the second Ctrl-C from ever
/// exiting. `seen` counts the interrupts so far; the first sets the cancel flag and calls `notify` (which only
/// wakes the thread that prints the note); every later one calls `abort` with [`ABORT_EXIT_CODE`]. All three
/// are injected so the logic is testable without a console.
pub fn on_interrupt(
    seen: &AtomicU32,
    cancel: &CancelToken,
    notify: impl FnOnce(),
    abort: impl FnOnce(i32),
) {
    if seen.fetch_add(1, Ordering::SeqCst) == 0 {
        cancel.cancel();
        notify();
    } else {
        abort(ABORT_EXIT_CODE);
    }
}

/// Install the process-wide handler. Call it before planning, so a Ctrl-C during a long scan is not lost.
///
/// The "cancelling" note is printed by a helper thread the handler wakes through a channel, so a blocked
/// stderr can stall only that thread. The second Ctrl-C calls `std::process::exit(3)` without writing anything.
pub fn install(cancel: CancelToken) -> Result<(), ctrlc::Error> {
    let (tx, rx) = mpsc::channel::<()>();
    // Without the helper thread the note is simply not printed; cancelling still works.
    let _ = std::thread::Builder::new()
        .name("sdc-ctrlc-note".into())
        .spawn(move || {
            if rx.recv().is_ok() {
                // A closed stderr pipe is ignored: the note is advisory.
                let _ = writeln!(std::io::stderr(), "{CANCELLING_NOTE}");
            }
        });
    let seen = Arc::new(AtomicU32::new(0));
    ctrlc::set_handler(move || {
        on_interrupt(
            &seen,
            &cancel,
            || {
                // Never blocks (unbounded channel); an error only means the helper thread is gone.
                let _ = tx.send(());
            },
            |c| std::process::exit(c),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn first_interrupt_cancels_and_notifies_later_ones_abort_with_3_without_notifying() {
        let (seen, cancel) = (AtomicU32::new(0), CancelToken::default());
        let (notes, exited) = (Cell::new(0u32), Cell::new(None));
        let interrupt = || {
            on_interrupt(
                &seen,
                &cancel,
                || notes.set(notes.get() + 1),
                |c| exited.set(Some(c)),
            )
        };
        interrupt();
        assert!(cancel.is_cancelled());
        assert_eq!(notes.get(), 1, "the first Ctrl-C wakes the note thread");
        assert_eq!(exited.get(), None, "the first Ctrl-C must not exit");
        interrupt();
        assert_eq!(exited.get(), Some(3));
        assert_eq!(notes.get(), 1, "the second Ctrl-C exits without any output");
        exited.set(None);
        interrupt();
        assert_eq!(exited.get(), Some(3), "every later Ctrl-C keeps aborting");
        assert_eq!(notes.get(), 1);
    }
}
