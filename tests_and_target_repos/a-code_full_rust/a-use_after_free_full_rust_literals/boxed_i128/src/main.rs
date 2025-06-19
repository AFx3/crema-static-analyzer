fn main(){

  let  b1: Box<i128> = Box::new(-170141183460469231731687303715884105728);
  println!("boxed i128 = {}", *b1);
      
  let raw: *mut i128 = Box::into_raw(b1); 

  //UAF

  let b1_again = unsafe { Box::from_raw(raw) };
  drop(b1_again);
  let val = unsafe { *raw }; // DEREF after free: UB
  println!("use-after-free value = {}", val);

    
}