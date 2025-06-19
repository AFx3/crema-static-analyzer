use std::{ffi::CString, process::abort};
use log::error;
extern crate log;
use std::ptr;

// cargo +nightly rustc -- -Z unpretty=mir-cfg > mir.dot


//double FREE

fn main() {
    // example list of callback names
    let mut callback_names = vec!["foo", "bar", "invalid\0name", "baz"];   
    // l'ultima ha un byte nullo interno: questo farà fallire la conversione in CString, perché in C terminazione \0 è considerata end stirng
    
       // pop off the last name
       let name = callback_names.pop().unwrap();

       let c_name_ptr: *mut i8 = match CString::new(name) {
        // success: hand back the raw C-pointer
        Ok(cstring) => cstring.into_raw(),
        // failure: log and return a null pointer
        Err(err) => {
            error!("Failed to create CString for {:?}: {}", name, err);
            abort();
        }};
        println!("Got raw C pointer: {:?}", c_name_ptr);


        // reconstruct the CString to free its memory
      
        unsafe {
            let _ = CString::from_raw(c_name_ptr);
        }
        

         
        unsafe {
            let _ = CString::from_raw(c_name_ptr);
        }



    /* 

       // attempt to make it a C string
       let c_name_ptr: *mut i8 = match CString::new(name) {
           // success: hand back the raw C-pointer
           Ok(cstring) => cstring.into_raw(),
           // failure: log and return a null pointer
           Err(err) => {
               error!("Failed to create CString for {:?}: {}", name, err);
               ptr::null_mut()
           }
       };*/
   
    
 
    /* 
    for name in callback_names {
        // Try to convert Rust &str into a C-compatible CString
        let c_name_ptr = match CString::new(name) {
            // ritorna Ok(cstring) se la stringa non contiene byte zero interni.
            Ok(cstring) => cstring.into_raw(),
            // eitorna Err(err) se trova uno o più \0 interni: in questo caso la conversione fallisce perché romperebbe la convenzione C di terminazione
            Err(err) => {
                error!("Failed to create CString for \"{}\": {}", name, err);
                println!("Error: {}, for the string `{}`", err, name);
                continue; // skip this name and move on
            }
        };

        // dopo la passo ad una ffi
        println!("Got raw C pointer: {:?}", c_name_ptr);

        // reconstruct the CString to free its memory
        unsafe {
            let _ = CString::from_raw(c_name_ptr);
        }
    }
    */
}
    