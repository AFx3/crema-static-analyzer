fn main(){

    let  b1: Box<u32> = Box::new(4294967295);
    println!("boxed u32 = {}", *b1);
        

    let raw: *mut u32 = Box::into_raw(b1);
   
    //UAF
    let b1_again = unsafe { Box::from_raw(raw) };
    drop(b1_again);
    let val = unsafe { *raw }; // DEREF after free: UB
    println!("use-after-free value = {}", val);
    
}