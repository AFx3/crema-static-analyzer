use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

fn main() {
    unsafe {
        let p = malloc(8);
        let q = malloc(16);
        black_box((p, q));
        if !p.is_null() { free(p); }
        if !q.is_null() { free(q); }
    }
}
