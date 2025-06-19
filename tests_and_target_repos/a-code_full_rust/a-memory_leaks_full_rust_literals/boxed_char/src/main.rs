// BOXED test

fn main(){
    
   // alloc
   let  b1: Box<char> = Box::new('a');
   println!("boxed char= {}", *b1);
        
   // ways to forget the ownership  : MEM LEAK  
 
   let raw: *mut char = Box::into_raw(b1);
   //let _ = Box::<char>::into_raw(b1); 
   //std::mem::forget(b1); 

        
}