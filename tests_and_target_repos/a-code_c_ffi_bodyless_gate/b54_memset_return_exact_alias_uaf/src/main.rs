use std::ffi::{c_int, c_void};
use std::hint::black_box;
unsafe extern "C" { fn memset(dst: *mut c_void, c: c_int, n: usize) -> *mut c_void; }
fn main() {
    let base = Box::into_raw(Box::new([b'A', b'B', b'C', 0_u8, 1, 2, 3, 4]));
    unsafe {
        let returned = memset(base.cast(), 0, 8);
        drop(Box::from_raw(base));
            black_box(*(returned as *const u8));
    }
}
