fn main(){

  let  b1: Box<u64> = Box::new(18_446_744_073_709_551_615);
  println!("boxed u64 = {}", *b1);
        
  let raw: *mut u64 = Box::into_raw(b1);


  // DOUBLE FREE
     
  let b1_again = unsafe { Box::from_raw(raw) };
  let b2 = unsafe { Box::from_raw(raw) };
    
}