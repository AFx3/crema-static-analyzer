// BOXED test
    
fn main(){
  // alloc
  let  b1: Box<i128> = Box::new(-170_141_183_460_469_231_731_687_303_715_884_105_728);
  println!("boxed i128 = {}", *b1);
        
  // ways to forget the ownership  : MEM LEAK  

  let raw: *mut i128 = Box::into_raw(b1); 
  // let _ = Box::<i128>::into_raw(b1);
  // std::mem::forget(b1);  
}