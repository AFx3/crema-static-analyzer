// BOXED test

fn main(){
  
  // alloc
  let  b1: Box<usize> = Box::new(18_446_744_073_709_551_615);
  println!("boxed usize= {}", b1);
          
  // ways to forget the ownership  : MEM LEAK  

  let raw: *mut usize = Box::into_raw(b1); 
  //let _ = Box::<usize>::into_raw(b1);
  // std::mem::forget(b1); 

}