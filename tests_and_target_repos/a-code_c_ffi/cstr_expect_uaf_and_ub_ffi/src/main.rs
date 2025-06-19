use std::{ffi::CString, process::abort};
use std::ptr;
use std::ffi::CStr;
use std::ffi::c_char;


//UAF
extern "C" {
  fn print_e_free(ptr: *mut c_char);
}

fn main(){

    let original = CString::new("ciao").expect("nisba");  
    let ptr: *mut c_char = original.into_raw();

  unsafe{ 

    let rust_str = CStr::from_ptr(ptr).to_string_lossy(); 
  
    println!("Stringa vista in Rust: {:?}", rust_str);
  }
    unsafe{
    print_e_free(ptr);
    println!("{}",*ptr);
    }
}
