use std::ffi::c_void;
use std::ptr::null_mut;

unsafe extern "C" {
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
    fn free(p: *mut c_void);
}

fn main() {
    unsafe {
        let q = realloc(null_mut(), 64);
        if q.is_null() {
            return;
        }
        let r = realloc(q, 128);
        if r.is_null() {
            free(q);
        } else {
            free(r);
        }
    }
}
