fn main(){
    
    let  b1: Box<u16> = Box::new(65535);
    println!("boxed u16 = {}", *b1);

    let raw: *mut u16 = Box::into_raw(b1);
    
    // DOUBLE FREE
    let b1_again = unsafe { Box::from_raw(raw) };
    let b2 = unsafe { Box::from_raw(raw) };
    
    
}