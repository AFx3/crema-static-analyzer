use std::ffi::c_void;


// NOTE: running multiple times with `cargo run` will lead to a double free error

extern "C" {
    fn cast(ptr: *mut c_void);
    fn rand() -> i32; 
}

fn main() {
    let z = Box::new(90);
    
    // convert Box to raw pointer
    let z_raw: *mut c_void = Box::into_raw(z) as *mut c_void;
    
    unsafe {
        // first free: via C function
        cast(z_raw);

        let num = rand() % 5; //calls c stdlib rand()
        println!("{}", num);
        
        if num >= 3 {
            // reconstruct a Box from the now-invalid pointer UB
            let _z_box = Box::from_raw(z_raw as *mut i32);
        } else {
            return;
        }
    }
}



// detected
/* 
use std::ffi::c_void;
extern "C" {
    fn cast_and_free_pointer(ptr: *mut c_void);
}

fn main() {
    // allocate memory on the heap and store it in a Box
    let z = Box::new(90);
    // convert Box to a raw pointer, relinquishing ownership
    let z_raw: *mut c_void = Box::into_raw(z) as *mut c_void;

    unsafe {
        // pass the raw pointer to an FFI function that frees the memory
        cast_and_free_pointer(z_raw);
        
        // reconstruct a Box from the same raw pointer
        // when this Box goes out of scope, Rust will attempt to free the memory againbleading to a double free error
       let _double_free_box = Box::from_raw(z_raw as *mut i32);
    }
   
    
}
    */