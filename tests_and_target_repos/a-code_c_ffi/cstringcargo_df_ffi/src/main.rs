use std::ffi::{CString, c_char};

// DOUBLE FREE

extern "C" {
    fn modify_and_free_string(ptr: *mut c_char);
}
fn main() {
/* CSTRING ::FROM
let c_string = CString::from(c"foo");
println!("Original string: {:?}", c_string);
let ptr = c_string.into_raw();
unsafe {
  // modify_and_free_string(ptr);

    // retake pointer to free memory
    let _ = CString::from_raw(ptr);


//USE AFTER FREE
   let mut a=*ptr;
    a+=1;
    println!("a is: {:?}", a);




    //print the ptr tieh println!

}
// TO DO CString::from_raw(ptr);
// double free
// free in rust 
*/


// CSTRING ::NEW

let c_string = CString::new("foo").unwrap();

println!("Original string: {:?}", c_string);
let ptr = c_string.into_raw();



// con option
/*
let original = "abc".to_string();
println!("Original string: {:?}", original);

let ptr = match CString::new(original) {
    Ok(cstring) => cstring.into_raw(),
    Err(_) => {
        eprintln!("CString conversion failed");
        return;
    }
};
*/
unsafe {


//modify_and_free_string(ptr);
modify_and_free_string(ptr);
let _ = CString::from_raw(ptr);


// DF
//let _ = CString::from_raw(ptr);

//USE AFTER FREE
/* 
let mut a=*ptr;
a+=1;
println!("a is: {:?}", a);

*/
}



}

