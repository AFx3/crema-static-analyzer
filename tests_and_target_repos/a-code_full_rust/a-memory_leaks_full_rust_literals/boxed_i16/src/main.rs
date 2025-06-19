// BOXED test

fn main(){
    
    // alloc
    let  b1: Box<i16> = Box::new(-32_768);
    println!("boxed i16 = {}", *b1);
    // ways to forget the ownership    

    let raw: *mut i16 = Box::into_raw(b1);
    //let _ = Box::<i16>::into_raw(b1); 
    //std::mem::forget(b1);
    

}