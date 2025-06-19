// BOXED u8 MEMORY LEAK

fn main(){
    
    // alloc
    let  b1: Box<u8> = Box::new(0);
    println!("boxed u8 = {}", *b1);

    // ways to forget the ownership   
    
    let raw: *mut u8 = Box::into_raw(b1);
    // let _ = Box::<u8>::into_raw(b1);
    //std::mem::forget(b1);

}