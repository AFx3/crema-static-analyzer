fn main(){

    let  b1: Box<u128> = Box::new(340_282_366_920_938_463_463_374_607_431_768_211_455);
    println!("boxed u128 = {}", *b1);
      

    let raw: *mut u128 = Box::into_raw(b1); 


    //UAF

    let b1_again = unsafe { Box::from_raw(raw) };
    drop(b1_again);
    let val = unsafe { *raw }; // DEREF after free: UB
    println!("use-after-free value = {}", val);

    
}