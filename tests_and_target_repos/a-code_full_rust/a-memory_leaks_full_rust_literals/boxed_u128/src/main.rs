// BOXED test

fn main(){
  // alloc
  let  b1: Box<u128> = Box::new(340_282_366_920_938_463_463_374_607_431_768_211_455);
  println!("boxed u128 = {}", *b1);
        
  // ways to forget the ownership  : MEM LEAK  
  let raw: *mut u128 = Box::into_raw(b1); 
  //  let _ = Box::<u128>::into_raw(b1);  
  // std::mem::forget(b1);

}