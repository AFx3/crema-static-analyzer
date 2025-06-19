use std::ffi::{c_void, c_int, c_char};

extern "C"{ 

    fn free_int(i: *mut c_int);
    fn free_str(s: *mut c_char);
    fn free_bool(b: *mut c_void);
}


fn main() {
     
    let a = Box::new(true);
    let raw = Box::into_raw(a) as *mut c_void;
    unsafe {
        free_bool(raw);
    }


    let b = Box::new(31);
    let raw = Box::into_raw(b) as *mut c_int;
    unsafe {
        free_int(raw);
    }

    let c = Box::new("test".to_string());
    let raw = Box::into_raw(c) as *mut c_char;
    unsafe {
        free_str(raw);
    }

}


