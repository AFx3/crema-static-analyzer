use std::ffi::{c_char, CString, c_void};
extern "C" {
    fn print_and(ptr: *mut c_char);
    fn print(ptr: *mut c_char);
    fn cast_and_free_pointer(ptr: *mut c_void);
}

fn main() {

       
    // DOUBLE FREE
    let strings = vec!["Alpha", "Beta", "Gamma"];
    for a in &strings {
        let c_str = CString::new(*a).expect("CString::new failed");
        let raw_pt = c_str.into_raw(); // rust gives up ownership, C must free it
        unsafe {
            print_and(raw_pt); // C will print and free it
            let _ = CString::from_raw(raw_pt); // reconstructing means a free drop() is inserted in the mir
        }
    }
    
 /* 

// DOUBLE FREE
let c_string = CString::new("foo").unwrap();
let ptr = c_string.into_raw();
unsafe {
    print_and(ptr); // C will print and free it
    let _ = CString::from_raw(ptr); // reconstructing means a free drop() is inserted in the mir
}
*/

/*TO DO 
let integers = vec![10, 20, 30];
    for num in &integers {
        // allocate a box on the heap
        let boxed = Box::new(*num);
        let raw_ptr = Box::into_raw(boxed); // get raw pointer
        unsafe {
            cast_and_free_pointer(raw_ptr as *mut c_void); // C will cast and free it
            
        }
        let b = unsafe { Box::from_raw(raw_ptr) };
    }

*/

    
}