use std::ffi::c_char;


extern "C"{ 

    fn free_str(i: *mut c_char);
}

fn main() {
     
    let a = Box::new(true);
    //forget the ownership of the string
    let raw = Box::into_raw(a) as *mut c_char;
    unsafe {
        free_str(raw);
    }

}


