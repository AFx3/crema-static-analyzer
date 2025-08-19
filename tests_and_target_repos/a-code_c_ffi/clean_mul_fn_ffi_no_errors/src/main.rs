use std::ffi::{CStr, CString};
use std::os::raw::c_char;

extern "C" {
    fn alloc_c_string(msg: *const c_char) -> *mut c_char;
    fn free_c_string(s: *mut c_char);
}

fn main() {
    let rust_str = CString::new("String!!!").unwrap();

    unsafe {
        let c_str = alloc_c_string(rust_str.as_ptr());
        println!("{}", CStr::from_ptr(c_str).to_str().unwrap());
        free_c_string(c_str); 
    }
}

