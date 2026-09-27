use std::ffi::{c_int, c_void};
use std::hint::black_box;

unsafe extern "C" {
    fn memset(s: *mut c_void, c: c_int, n: usize) -> *mut c_void;
}

fn main() {
    let p = Box::into_raw(Box::new([0u8; 16]));
    unsafe {
        drop(Box::from_raw(p));
        black_box(memset(p.cast::<c_void>(), 0, 16));
    }
}
