fn main(){

    let  b1: Box<u16> = Box::new(65535);
    println!("boxed u16 = {}", *b1);
        
    let raw: *mut u16 = Box::into_raw(b1);

    let b1_again = unsafe { Box::from_raw(raw) };

    
    //UAF
    drop(b1_again);
    let val = unsafe { *raw }; // DEREF after free: UB
    println!("use-after-free value = {}", val);
    
}