use std::ffi::c_void;
use std::ptr::null_mut;

unsafe extern "C" {
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
}

fn main() {
    unsafe {
        let p: *mut u8 = null_mut();
        let v = p as *mut c_void;
        let _q = realloc(v, 64);
    }
}
