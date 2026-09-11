use std::ffi::CStr;
use std::os::raw::c_char;

extern "C" {
    fn c_static_string() -> *const c_char;
}

fn main() {
    unsafe {
        let p = c_static_string();
        if p.is_null() { return; }
        println!("{}", CStr::from_ptr(p).to_string_lossy());
    }
}
