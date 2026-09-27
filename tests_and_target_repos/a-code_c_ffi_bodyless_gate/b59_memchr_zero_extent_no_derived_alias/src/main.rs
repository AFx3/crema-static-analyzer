use std::ffi::{c_int, c_void};
use std::hint::black_box;
unsafe extern "C" { fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void; }
fn main() {
    let base = Box::into_raw(Box::new([b'A', b'B', b'C', 0_u8, 1, 2, 3, 4]));
    unsafe {
        let returned = memchr(base.cast(), b'B' as c_int, 0);
        black_box(returned);
        drop(Box::from_raw(base));
    }
}
