   // BOXED test

fn main(){
   // alloc
   
   let  b1: Box<bool> = Box::new(false);
   println!("boxed bool= {}", b1);
        
   // ways to forget the ownership: MEM LEAK  

   let raw: *mut bool = Box::into_raw(b1); 
   //let _ = Box::<bool>::into_raw(b1);
   //std::mem::forget(b1); 
   
   
}
