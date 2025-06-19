fn main(){
    
  let  b1: Box<u32> = Box::new(4294967295);
  println!("boxed u32 = {}", *b1);
 
  let raw: *mut u32 = Box::into_raw(b1);
   
  // DOUBLE FREE
     
  let b1_again = unsafe { Box::from_raw(raw) };
  let b2 = unsafe { Box::from_raw(raw) };
  
    
}