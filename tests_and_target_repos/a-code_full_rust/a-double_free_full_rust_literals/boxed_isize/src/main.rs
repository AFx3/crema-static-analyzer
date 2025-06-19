fn main(){

   let  b1: Box<isize> = Box::new(-9_223_372_036_854_775_808);
   println!("boxed isize= {}", *b1);

   let raw: *mut isize = Box::into_raw(b1); 


   // DOUBLE FREE 
   let b1_again = unsafe { Box::from_raw(raw) };
   let b2 = unsafe { Box::from_raw(raw) };

}