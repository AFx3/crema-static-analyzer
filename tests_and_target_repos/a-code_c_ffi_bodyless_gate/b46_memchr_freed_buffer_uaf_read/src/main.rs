use std::ffi::{c_int, c_void};
use std::hint::black_box;

unsafe extern "C" {
    fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void;
}

fn main() {
    let buf = Box::into_raw(Box::new([1_u8, 2, 3, 4, 5, 6, 7, 8]));
    unsafe {
        drop(Box::from_raw(buf));
        black_box(memchr(buf.cast::<u8>().cast::<c_void>(), 4, 8));
    }
}
