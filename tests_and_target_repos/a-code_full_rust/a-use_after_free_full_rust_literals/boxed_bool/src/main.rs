fn main(){

   let  b1: Box<bool> = Box::new(true);
   println!("boxed bool= {}", *b1);

   let raw: *mut bool = Box::into_raw(b1); 

   // UAF
   let b1_again = unsafe { Box::from_raw(raw) };
   drop(b1_again);
   let val = unsafe { *raw }; // DEREF after free: UB
   println!("use-after-free value = {}", val);

}
