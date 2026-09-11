use std::os::raw::c_int;

extern "C" {
    fn c_alloc_i32(value: c_int) -> *mut c_int;
    fn c_free_i32(p: *mut c_int);
}

fn main() {
    unsafe {
        let p = c_alloc_i32(34);
        if p.is_null() {
            return;
        }

        // The same wrapper is inlined twice at distinct Rust call sites.
        c_free_i32(p);
        c_free_i32(p);
    }
}
