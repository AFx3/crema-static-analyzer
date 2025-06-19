fn main(){
    
    let  b1: Box<u8> = Box::new(255);
    println!("boxed u8 = {}", *b1);
  
    let raw: *mut u8 = Box::into_raw(b1);
   
    // DOUBLE FREE
    let b1_again = unsafe { Box::from_raw(raw) };
    let b2 = unsafe { Box::from_raw(raw) };
}