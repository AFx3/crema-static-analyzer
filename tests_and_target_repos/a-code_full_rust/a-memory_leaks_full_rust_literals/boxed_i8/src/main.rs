// BOXED u8 MEMORY LEAK

fn main(){
    
    // alloc
    let  b1: Box<i8> = Box::new(-1);
    
    println!("boxed i8 = {}", *b1);
        
    // ways to forget the ownership  
    
    let raw: *mut i8 = Box::into_raw(b1);  
    //let _ = Box::<i8>::into_raw(b1);
    // std::mem::forget(b1);

}