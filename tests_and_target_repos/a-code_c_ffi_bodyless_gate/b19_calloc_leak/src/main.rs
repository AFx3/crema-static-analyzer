use std::ffi::c_void;
use std::hint::black_box;

unsafe extern "C" {
    fn calloc(nmemb: usize, size: usize) -> *mut c_void;
}

fn main() {
    unsafe {
        black_box(calloc(4, 16));
    }
}
