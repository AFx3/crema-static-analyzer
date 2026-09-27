use std::ffi::c_void;
use std::ptr::null_mut;

unsafe extern "C" {
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
}

fn main() {
    unsafe {
        let p: *mut c_void = null_mut();
        let r = p;
        let _q = realloc(r, 64);
    }
}
