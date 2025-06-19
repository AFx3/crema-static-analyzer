fn main(){
    
  let  b1: Box<usize> = Box::new(18_446_744_073_709_551_615);
  println!("boxed usize= {}", b1);
        
  let raw: *mut usize = Box::into_raw(b1); 


  // DOUBLE FREE 
  let b1_again = unsafe { Box::from_raw(raw) };
  let b2 = unsafe { Box::from_raw(raw) };
  
    
}