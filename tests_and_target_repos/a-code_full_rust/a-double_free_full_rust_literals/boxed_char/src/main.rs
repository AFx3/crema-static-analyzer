fn main(){

   let  b1: Box<char> = Box::new('a');
   println!("boxed char= {}", *b1);
        
   let raw: *mut char = Box::into_raw(b1); 

   // DOUBLE FREE 
   let b1_again = unsafe { Box::from_raw(raw) };
   let b2 = unsafe { Box::from_raw(raw) };
        
    
}