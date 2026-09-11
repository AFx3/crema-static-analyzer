use std::ffi::CStr;
use std::os::raw::c_char;

extern "C" {
    fn c_alloc_string() -> *mut c_char;
    fn c_free_string(p: *mut c_char);
}

fn main() {
    unsafe {
        let p = c_alloc_string();
        if p.is_null() { return; }
        println!("{}", CStr::from_ptr(p).to_string_lossy());
        c_free_string(p);
    }
}
