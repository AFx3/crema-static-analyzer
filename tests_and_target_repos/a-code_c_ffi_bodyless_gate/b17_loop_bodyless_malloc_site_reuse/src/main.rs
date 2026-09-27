use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

fn main() {
    for _ in 0..2 {
        unsafe {
            let p = malloc(8);
            black_box(p);
            if !p.is_null() { free(p); }
        }
    }
}
