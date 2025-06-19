use std::ptr;
use std::ffi::c_char;
use std::ffi::CString;
// UAF
fn main(){
    let original = CString::new("ciao").expect("interior NUL!");
    let ptr: *mut c_char = original.into_raw();

  
  let _ = unsafe { Box::from_raw(ptr) };
  let rust_str  = unsafe { ptr::read(ptr) };
    println!("Ho letto: {:?}", rust_str);
 
}