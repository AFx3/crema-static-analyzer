// BOXED test

fn main(){
    
  // alloc
  let  b1: Box<i64> = Box::new(-9_223_372_036_854_775_808); 
  println!("boxed i64 = {}", *b1);
        
  // ways to forget the ownership  : MEM LEAK  

  let raw: *mut i64 = Box::into_raw(b1);
  //let _ = Box::<i64>::into_raw(b1);
  // std::mem::forget(b1);

  
}