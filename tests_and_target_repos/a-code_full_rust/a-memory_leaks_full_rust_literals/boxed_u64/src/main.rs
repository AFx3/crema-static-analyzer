// BOXED test

fn main(){

  // alloc
  let  b1: Box<u64> = Box::new(18_446_744_073_709_551_615);
  println!("boxed u64 = {}", *b1);

  // ways to forget the ownership  : MEM LEAK  
  
  let raw: *mut u64 = Box::into_raw(b1);
  //let _ = Box::<u64>::into_raw(b1);
  // std::mem::forget(b1);



}