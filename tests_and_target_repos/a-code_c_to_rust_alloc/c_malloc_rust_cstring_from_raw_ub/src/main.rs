use std::ffi::CString;
use std::os::raw::c_char;

extern "C" {
    fn c_alloc_string() -> *mut c_char;
}

fn main() {
    unsafe {
        let p = c_alloc_string();
        if p.is_null() { return; }
        let owned = CString::from_raw(p);
        println!("{:?}", owned);
    }
}
