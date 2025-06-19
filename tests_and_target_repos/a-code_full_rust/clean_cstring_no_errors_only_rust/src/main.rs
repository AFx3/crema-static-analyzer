use std::{ffi::CString, process::abort};

use std::ptr;
use std::ffi::CStr;
use std::ffi::c_char;
// cargo +nightly rustc -- -Z unpretty=mir-cfg > mir.dot
   

    
fn main() {
    // 1) Build a CString (owns its buffer)
    let c_string = CString::new("ciao").unwrap();

    // 2) Grab a raw pointer to its buffer (no ownership change yet)
    let ptr = c_string.into_raw();
    //let ptr = c_string.as_ptr() as *mut c_char;

    unsafe {
        // 3a) First reclaim + free
        let reclaimed1 = CString::from_raw(ptr);
        drop(reclaimed1);

        // 3b) Second reclaim + free -> **DOUBLE FREE**
        //let reclaimed2 = CString::from_raw(ptr);
        //drop(reclaimed2);
    }

 
}