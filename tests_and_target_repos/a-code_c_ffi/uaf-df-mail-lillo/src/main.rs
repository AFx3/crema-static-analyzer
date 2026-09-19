extern "C" { fn c_free(p: *mut i32); }

  fn main() {
      let a = Box::new(1);
      let p = Box::into_raw(a);              // ownership dimenticata

      if (2>1) {
          unsafe { c_free(p) };              // il C libera un'allocazione Rust
          unsafe { let v = *p; }             // (1) USE-AFTER-FREE
      } else {
          let b = unsafe { Box::from_raw(p) };
          drop(b);                           // liberata dal Rust
      }

      let c = unsafe { Box::from_raw(p) };
      drop(c);                               // (2) DOUBLE FREE su entrambi i rami

      let d = Box::new(2);
      let q = Box::into_raw(d);              // (3) NEVER FREE
  }
