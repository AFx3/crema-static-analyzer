use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
}

fn main() {
    unsafe {
        let p = malloc(32);
        black_box(p);
    }
}
