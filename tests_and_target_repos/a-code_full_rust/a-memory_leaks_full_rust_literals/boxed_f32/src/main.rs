// BOXED test

fn main(){
    
   // alloc
   let  b1: Box<f32> = Box::new(-113.75);
   println!("boxed f32 = {}", *b1);
        
   // ways to forget the ownership  : MEM LEAK
   
   let raw: *mut f32 = Box::into_raw(b1);  
   // let _ = Box::<f32>::into_raw(b1); 
   // std::mem::forget(b1);

       
}