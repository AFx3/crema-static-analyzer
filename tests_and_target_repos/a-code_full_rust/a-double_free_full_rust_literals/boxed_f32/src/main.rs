fn main(){

   let  b1: Box<f32> = Box::new(-113.75);
   println!("boxed f32 = {}", *b1);
        
   let raw: *mut f32 = Box::into_raw(b1); 

   // DOUBLE FREE 
   let b1_again = unsafe { Box::from_raw(raw) };
   let b2 = unsafe { Box::from_raw(raw) };
    

}