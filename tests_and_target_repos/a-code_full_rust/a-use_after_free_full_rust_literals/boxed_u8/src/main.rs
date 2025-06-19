fn main(){
    
    let  b1: Box<u8> = Box::new(0);
    println!("boxed u8 = {}", *b1);

    let raw: *mut u8 = Box::into_raw(b1);


    //UAF
    let b1_again = unsafe { Box::from_raw(raw) };
    drop(b1_again);
    let val = unsafe { *raw }; // DEREF after free: UB!
    println!("use-after-free value = {}", val);

}