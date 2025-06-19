fn main(){

    let  b1: Box<i16> = Box::new(-32_768);
    println!("boxed i16 = {}", *b1);
        
    // ways to forget the ownership    
    let raw: *mut i16 = Box::into_raw(b1);


    // DOUBLE FREE
     
    let b1_again = unsafe { Box::from_raw(raw) };
    let b2 = unsafe { Box::from_raw(raw) };
    

    
}