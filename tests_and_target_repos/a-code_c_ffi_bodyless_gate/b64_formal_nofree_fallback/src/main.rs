unsafe extern "C" { fn calloc(n: usize, size: usize) -> *mut u8; fn free(p: *mut u8); }
fn main() { unsafe { let p = calloc(1, 4); std::hint::black_box(p); free(p); } }
