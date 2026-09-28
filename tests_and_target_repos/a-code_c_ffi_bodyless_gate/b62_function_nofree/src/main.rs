unsafe extern "C" { fn malloc(n: usize) -> *mut u8; fn free(p: *mut u8); }
fn main() { unsafe { let p = malloc(4); std::hint::black_box(p); free(p); } }
