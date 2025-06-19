fn main(){

  let  b1: Box<f64> = Box::new(2.2204460492503131E-16f64);
  println!("boxed f64 = {}", *b1);

  let raw: *mut f64 = Box::into_raw(b1); 

  //UAF
  let b1_again = unsafe { Box::from_raw(raw) };
  drop(b1_again);
  let val = unsafe { *raw }; // DEREF after free: UB
  println!("use-after-free value = {}", val);

    
}