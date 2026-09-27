use std::ffi::{c_char, c_int};
use std::hint::black_box;

unsafe extern "C" {
    fn strchr(s: *const c_char, c: c_int) -> *mut c_char;
}

fn main() {
    let string = Box::into_raw(Box::new([b'A', b'B', b'C', 0_u8]));
    unsafe {
        drop(Box::from_raw(string));
        black_box(strchr(string.cast::<u8>().cast::<c_char>(), b'B' as c_int));
    }
}
