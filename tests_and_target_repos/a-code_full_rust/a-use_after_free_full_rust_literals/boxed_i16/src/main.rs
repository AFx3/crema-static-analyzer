fn main(){

    let  b1: Box<i16> = Box::new(-32_768);
    println!("boxed i16 = {}", *b1);


    let raw: *mut i16 = Box::into_raw(b1);
    let b1_again = unsafe { Box::from_raw(raw) };


    //UAF

    drop(b1_again);
    let val = unsafe { *raw }; // DEREF after free: UB
    println!("use-after-free value = {}", val);

}