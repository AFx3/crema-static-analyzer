
use std::ffi::{CString, CStr};
use std::os::raw::{c_char, c_void};

unsafe extern "C" fn open_url(
    url: *mut c_char,
    _userdata: *mut c_void,
) -> *mut c_char {
    open::that(CStr::from_ptr(url).to_str().unwrap()).unwrap();
    CString::new("").unwrap().into_raw()
}

fn main() {

    let original = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    let mut c_url = CString::new(original).expect("CString::new failed");


    let ret_ptr = unsafe {
        open_url(c_url.as_ptr() as *mut c_char, std::ptr::null_mut())
    };


    let ret_str = unsafe { CStr::from_ptr(ret_ptr).to_str().unwrap() };
    println!("open_url returned: {:?}", ret_str);

    
}
    
