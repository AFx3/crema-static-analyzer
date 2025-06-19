fn main(){

    let  b1: Box<bool> = Box::new(true);
    println!("boxed bool= {}", *b1);

    let raw: *mut bool = Box::into_raw(b1); 

   // DOUBLE FREE 

   let b1_again = unsafe { Box::from_raw(raw) };
   let b2 = unsafe { Box::from_raw(raw) };
     
}
