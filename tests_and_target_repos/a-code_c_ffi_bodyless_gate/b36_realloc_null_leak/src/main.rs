use std::ffi::c_void;
use std::ptr::null_mut;

unsafe extern "C" {
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
}

fn main() {
    unsafe {
        let _q = realloc(null_mut(), 64);
    }
}
