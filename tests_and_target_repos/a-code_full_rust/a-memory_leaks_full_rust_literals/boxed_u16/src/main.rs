// BOXED test

fn main(){
    // alloc
    let  b1: Box<u16> = Box::new(65535);
    println!("boxed u16 = {}", *b1);
        
    // ways to forget the ownership    

    let raw: *mut u16 = Box::into_raw(b1);
    //let _ = Box::<u16>::into_raw(b1);
    //std::mem::forget(b1);
    
}