use std::ffi::{c_int, c_void};
use std::hint::black_box;

unsafe extern "C" {
    fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void;
}

fn main() {
    let buf = Box::new([1_u8, 2, 3, 4]);
    unsafe {
        black_box(memchr(buf.as_ptr().cast::<c_void>(), 3, 0));
    }
    drop(buf);
}
