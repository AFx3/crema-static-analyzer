use std::{ffi::CString, process::abort};
use std::ptr;
use std::ffi::CStr;
use std::ffi::c_char;

/* 
// FAR VEDERE SE TORNA A LILLO
fn main(){
  //let original = CString::new("ciao").unwrap();
  let original = CString::new("ciao").expect("interior NUL!");
  
  let ptr: *mut c_char = original.into_raw();

unsafe{ 
  let rust_str = CStr::from_ptr(ptr).to_string_lossy(); // ATTENZIONE! CHIAMA IL FINTO DESTRUCTOR DROP 

//unsafe{ let rust_str = CStr::from_ptr(ptr);
  println!("Ho letto: {:?}", rust_str);
}
  unsafe{
   // CHIAMA IL DESTRUCTOR DROP
  // 1a reclamation + free
 let c1 = CString::from_raw(ptr as *mut c_char);
  // 2a reclamation + free -> DF 
 // let c2 = CString::from_raw(ptr as *mut c_char);
  }
}*/



//DOUBLE FREE
extern "C" {
  fn print_e_free(ptr: *mut c_char);
}

fn main(){
    //let original = CString::new("ciao").unwrap();
    let original = CString::new("ciao").expect("nisba");
  
    let ptr: *mut c_char = original.into_raw();

  unsafe{ 
    let rust_str = CStr::from_ptr(ptr).to_string_lossy(); // CHIAMA IL DESSTRUCTOR DROP
  // SENZA DESTRUCTOR e quindi never free 
  //unsafe{ let rust_str = CStr::from_ptr(ptr);
    println!("Stringa vista in Rust: {:?}", rust_str);
  }
    unsafe{
    print_e_free(ptr);
     // CHIAMA IL DESTRUCTOR DROP
    // 1a reclamation + free
    let c1 = CString::from_raw(ptr as *mut c_char);
    // 2a reclamation + free -> DF 
    //let c2 = CString::from_raw(ptr as *mut c_char);
    }
}










/* 
// cargo +nightly rustc -- -Z unpretty=mir-cfg > mir.dot
unsafe fn double_free_example(name: *const c_char) {
    // 1. Leggo la stringa C come Cow<str> (lossy conversion)
    let rust_str = CStr::from_ptr(name).to_string_lossy();
    println!("Ho letto: {}", rust_str);

    // 2. Prima reclamation + free
    //let c1 = CString::from_raw(name as *mut c_char);
    

    // 3. Seconda reclamation + free → **DOUBLE FREE UB**
    //let c2 = CString::from_raw(name as *mut c_char);
    
}
fn main() {
    // Creo la CString originale e ne prendo possesso con into_raw()
    let original = CString::new("ciao").expect("interior NUL!");
    let ptr: *mut c_char = original.into_raw();

    // Qui chiamo la funzione UB che fa doppio free
    unsafe {
        double_free_example(ptr);
    }
}
*/