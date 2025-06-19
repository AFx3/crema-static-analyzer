// BOXED test

fn main(){ 
  
  // alloc
  let  b1: Box<i32> = Box::new(-2_147_483_648);
  println!("boxed i32 = {}", *b1);
        
  // ways to forget the ownership 

  let raw: *mut i32 = Box::into_raw(b1);
  // let _ = Box::<i32>::into_raw(b1);
  // std::mem::forget(b1);
  
}