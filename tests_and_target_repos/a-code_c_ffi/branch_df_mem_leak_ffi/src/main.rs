use std::ffi::{c_char, CString};
extern "C" {
    fn print_and(ptr: *mut c_char);
    fn print(ptr: *mut c_char);
}

fn main() {
    
    let a = 4;
    let b = 10;

    let c_string = CString::new("TEST").unwrap();
    //println!("Original string: {:?}", c_string);
    let ptr = c_string.into_raw();
    unsafe{print_and(ptr);} // C will print and free it

    // DOUBLE FREE
    if a<b {
        println!("Unsafe branch -> Double Free.");
        // reconstruct the pointer (string already freed in c -> DOUBLE FREE)
        unsafe{let _ = CString::from_raw(ptr);} // reconstructiong means a free drop() is inserted in the mir
        println!("Reconstructed string: {:?}", ptr);
    } else {
        println!("Safe branch -> NO Double Free.");
    }   


 
    // MEMORY LEAK
    let c_str = CString::new("TEST ML").unwrap();
    println!("Original string: {:?}", c_str);
    let ptr = c_str.into_raw();
    unsafe {
        print(ptr);
    }
    



    /* 

// DOUBLE FREE
    let b = Box::new(10);
    let b_raw = Box::into_raw(b);

    let b = unsafe { Box::from_raw(b_raw) };
    let c = unsafe { Box::from_raw(b_raw) };
    */
    
}
    