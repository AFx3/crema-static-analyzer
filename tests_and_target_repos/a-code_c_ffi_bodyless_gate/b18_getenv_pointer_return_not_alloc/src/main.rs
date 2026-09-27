use std::ffi::{c_char, CString};
use std::hint::black_box;

unsafe extern "C" {
    fn getenv(name: *const c_char) -> *mut c_char;
}

fn main() {
    let name = CString::new("PATH").unwrap();
    unsafe {
        black_box(getenv(name.as_ptr()));
    }
}
