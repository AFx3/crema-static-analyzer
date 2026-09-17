//! B1.2 reproducer — RUSTSEC-2026-0282 (analyzable variant)
//!
//! shared trigger:
//!   AlignedBox<[PanicOnDrop]> with 4 elements, shrink to 1.
//!   Element with id == 2 panics on the first call to its Drop.
//!   No catch_unwind, no panic hook: the panic propagates to the main
//!   thread boundary, and the observation is by exit code.
//!
//! Exit codes:
//!   2   : no panic at all (unexpected)
//!   101 : panic propagated cleanly (expected on FIXED 0.3.1)
//!   134 : SIGABRT — glibc detected double free during unwind (expected on VULNERABLE 0.3.0)

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use aligned_box::AlignedBox;

static PANICKED: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU32 = AtomicU32::new(0);

struct PanicOnDrop {
    id: u32,
    inner: Box<u32>,
}

impl Default for PanicOnDrop {
    fn default() -> Self {
        PanicOnDrop {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            inner: Box::new(0),
        }
    }
}

impl Drop for PanicOnDrop {
    fn drop(&mut self) {
        if self.id == 2 && !PANICKED.swap(true, Ordering::SeqCst) {
            panic!("intentional panic in element id==2 Drop");
        }
    }
}

fn main() {
    let mut b: AlignedBox<[PanicOnDrop]> =
        AlignedBox::slice_from_default(128, 4).unwrap();

    let _ = b.realloc_with_default(1);

    // Only reached if the intended panic did not fire.
    eprintln!("UNEXPECTED: no panic");
    std::process::exit(2);
}
