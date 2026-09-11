use std::ffi::c_void;
use std::os::raw::c_int;

extern "C" {
    fn c_alloc_i32(value: c_int) -> *mut c_int;
    fn free(p: *mut c_void);
}

fn main() {
    unsafe {
        let p = c_alloc_i32(7);
        if p.is_null() {
            return;
        }

        println!("{}", *p);
        free(p.cast::<c_void>());
    }
}
