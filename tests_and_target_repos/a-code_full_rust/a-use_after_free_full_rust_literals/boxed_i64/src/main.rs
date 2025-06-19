fn main(){

  let  b1: Box<i64> = Box::new(-9_223_372_036_854_775_808);
  println!("boxed i64 = {}", *b1); 

  let raw: *mut i64 = Box::into_raw(b1);

  //UAF
  let b1_again = unsafe { Box::from_raw(raw) };
  drop(b1_again);
  let val = unsafe { *raw }; // DEREF after free: UB
  println!("use-after-free value = {}", val);
    

}