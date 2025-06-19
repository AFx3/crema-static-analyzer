// BOXED test

fn main(){

   // alloc
   let  b1: Box<isize> = Box::new(-9_223_372_036_854_775_808);
   println!("boxed isize= {}", *b1);
      
   // ways to forget the ownership  : MEM LEAK  

   let raw: *mut isize = Box::into_raw(b1);  
   //let _ = Box::<isize>::into_raw(b1);
   // std::mem::forget(b1); 
    
}