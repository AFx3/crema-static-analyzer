use std::ffi::{c_char, c_int};
use std::hint::black_box;
unsafe extern "C" { fn strchr(s: *const c_char, c: c_int) -> *mut c_char; }
fn main() {
    let base = Box::into_raw(Box::new([b'A', b'B', b'C', 0_u8, 1, 2, 3, 4]));
    unsafe {
        let returned = strchr(base.cast(), b'B' as c_int);
        if !returned.is_null() {
            drop(Box::from_raw(base));
            black_box(*(returned as *const u8));
        } else { drop(Box::from_raw(base)); }
    }
}
