// BOXED test

fn main(){
    
  // alloc
  let  b1: Box<u32> = Box::new(4294967295);
  println!("boxed u32 = {}", *b1);
        
  // ways to forget the ownership  : MEM LEAK

  let raw: *mut u32 = Box::into_raw(b1);  
  //let _ = Box::<u32>::into_raw(b1);
  // std::mem::forget(b1);
  
}