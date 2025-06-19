use std::ffi::c_void;
use std::os::raw::c_int;

extern "C"{ 
    fn free_str(s: *mut c_void);
    fn free_int(i: *mut c_int);
}

fn main() {
     
    let b = Box::new(3);
    let raw = Box::into_raw(b) as *mut c_int;
        unsafe {
            free_int(raw);
        }
    
}


