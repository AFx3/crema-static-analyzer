use std::ffi::c_void;
use std::os::raw::c_int;

extern "C"{ 
    fn free_bool(s: *mut c_void);
}

fn main() {
     
    let b = Box::new(true);
    let raw = Box::into_raw(b) as *mut c_void;
        unsafe {
            free_bool(raw);
        }
    
}


