// BOXED test

fn main(){
    
  // alloc
  let  b1: Box<f64> = Box::new(2.2204460492503131E-16f64); // machine epsilon value for f64
  println!("boxed f64 = {}", b1);
        
  // ways to forget the ownership  : MEM LEAK  
  
  let raw: *mut f64 = Box::into_raw(b1); 
  //std::mem::forget(b1);
  //let _ = Box::<f64>::into_raw(b1); 
    
}