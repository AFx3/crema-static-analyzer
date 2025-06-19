fn main(){

    let  b1: Box<usize> = Box::new(18_446_744_073_709_551_615);
    println!("boxed usize= {}", *b1);

    let raw: *mut usize = Box::into_raw(b1); 

    //UAF

    let b1_again = unsafe { Box::from_raw(raw) };
    drop(b1_again);
    let val = unsafe { *raw }; // DEREF after free: UB
    println!("use-after-free value = {}", val);


}