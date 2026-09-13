use std::hint::black_box;
use std::ptr;

extern "C" {
    fn crema_v6i_alloc(n: usize) -> *mut u8;
    fn crema_v6i_free(p: *mut u8);
}

fn main() {
    let p = unsafe { crema_v6i_alloc(8) };
    if p.is_null() {
        return;
    }

    // Runtime-dependent branch keeps all three witness paths in the static ICFG.
    // The gate never executes this program; it only builds and analyzes it.
    match std::env::args().len() % 3 {
        0 => unsafe {
            // Concrete double-free witness: same C malloc allocation, two C frees.
            crema_v6i_free(p);
            crema_v6i_free(p);
        },
        1 => unsafe {
            // Concrete cross-language UAF witness: C free, then Rust read.
            crema_v6i_free(p);
            black_box(ptr::read_volatile(p));
        },
        _ => {
            // Concrete leak witness: the raw pointer is deliberately not freed.
            black_box(p);
        }
    }
}
