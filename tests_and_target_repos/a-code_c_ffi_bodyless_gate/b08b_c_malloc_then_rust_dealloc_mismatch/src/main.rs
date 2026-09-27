use std::alloc::{dealloc, Layout};
use std::ffi::c_void;

unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
}

fn main() {
    unsafe {
        let p = malloc(32) as *mut u8;
        if p.is_null() {
            return;
        }
        let layout = Layout::from_size_align(32, 8).unwrap();
        dealloc(p, layout);
    }
}
