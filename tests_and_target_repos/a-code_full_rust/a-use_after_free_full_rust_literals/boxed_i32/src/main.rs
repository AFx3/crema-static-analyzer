fn main(){

  let  b1: Box<i32> = Box::new(-2_147_483_648);
  println!("boxed i32 = {}", *b1);
      
  let raw: *mut i32 = Box::into_raw(b1);

  let b1_again = unsafe { Box::from_raw(raw) };

  //UAF
  drop(b1_again);
  let val = unsafe { *raw }; // DEREF after free: UB
  println!("use-after-free value = {}", val);
    
}