fn main(){
    
  let  b1: Box<i32> = Box::new(-2_147_483_648);
  println!("boxed i32 = {}", *b1);
      
  let raw: *mut i32 = Box::into_raw(b1);

  // DOUBLE FREE     
  let b1_again = unsafe { Box::from_raw(raw) };
  let b2 = unsafe { Box::from_raw(raw) };
    
}