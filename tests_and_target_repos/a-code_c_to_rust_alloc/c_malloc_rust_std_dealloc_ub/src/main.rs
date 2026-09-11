use std::alloc::{dealloc, Layout};
use std::ffi::c_void;

extern "C" {
    fn c_alloc_16() -> *mut c_void;
}

fn main() {
    unsafe {
        let p = c_alloc_16() as *mut u8;
        if p.is_null() { return; }
        let layout = Layout::from_size_align(16, 8).unwrap();
        dealloc(p, layout);
    }
}
