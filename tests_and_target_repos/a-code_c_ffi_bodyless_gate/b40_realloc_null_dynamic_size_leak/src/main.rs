use std::ffi::c_void;
use std::ptr::null_mut;

unsafe extern "C" {
    fn realloc(p: *mut c_void, n: usize) -> *mut c_void;
}

fn main() {
    // Size is genuinely runtime-derived.  The NULL source is constructed only
    // after the std::process::id() call so the conservative call-kill rule does
    // not erase the MUST-null proof before realloc.
    let n = std::process::id() as usize;
    unsafe {
        let p: *mut c_void = null_mut();
        let _q = realloc(p, n);
    }
}
