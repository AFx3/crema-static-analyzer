use std::ffi::c_void;
use std::os::raw::c_int;
use std::ptr;

extern "C" {
    fn c_alloc_i32(value: c_int) -> *mut c_int;
    fn free(p: *mut c_void);
}

fn main() {
    unsafe {
        let p = c_alloc_i32(11);
        if p.is_null() {
            return;
        }

        free(p.cast::<c_void>());

        // Deliberate use-after-free.
        let _after_free = ptr::read(p);
    }
}
