fn main(){

   let  b1: Box<char> = Box::new('a');
   println!("boxed char= {}", *b1);

   let raw: *mut char = Box::into_raw(b1); 


   //UAF

   let b1_again = unsafe { Box::from_raw(raw) };
   drop(b1_again);
   let val = unsafe { *raw }; // DEREF after free: UB
   println!("use-after-free value = {}", val);


}