use libc::c_void;
use std::mem;
use std::ptr;

extern "C" {
    pub fn register_cb(cb: extern "C" fn(_: *mut c_void), arg: *mut c_void);
    pub fn trigger_cb();
}

struct Closure {
    cb: *mut *mut dyn FnMut(),
}

impl Closure {
    fn new() -> Self {
        Closure {
            cb: ptr::null_mut(),
        }
    }

    fn dealloc_cb(&mut self) {
        if !self.cb.is_null() {
            unsafe {
                // de-register closure before deallocation
                // otherwise we have dangling pointer
                register_cb(
                    mem::transmute::<*const c_void, extern "C" fn(_: *mut c_void)>(ptr::null()),
                    ptr::null_mut(),
                );
                let _box_fnmut = Box::from_raw(*self.cb);
                let _box_box_fnmut = Box::from_raw(self.cb);
                self.cb = ptr::null_mut();
            }
        }
    }

    fn register(&mut self, closure: impl FnMut() + 'static) {
        extern "C" fn call_closure(data: *mut c_void) {
            unsafe {
                let cb = data as *mut *mut dyn FnMut();
                (**cb)();
            }
        }

        self.dealloc_cb();

        // Wrap closure into Box and get the raw pointer of it.
        //
        // If we don't specify the type of Box then we will get a Box that points
        // to the actual closure instead of its `FnMut()` implementation. The result
        // will be an incorrect pointer casting in `call_closure` and a crash.
        let fnmut_box: Box<dyn FnMut()> = Box::new(closure);
        let fnmut_ptr = Box::into_raw(fnmut_box);

        // `fnmut_ptr` has type `*mut dyn FnMut()` which is a fat pointer.
        // Fat pointer can not be passed as `*c_void` because they have bigger size, so
        // we need to wrap it to one more Box to get a regular pointer.
        self.cb = Box::into_raw(Box::new(fnmut_ptr));

        unsafe {
            register_cb(call_closure, self.cb as *mut c_void);
        }
    }

    fn trigger(&self) {
        if !self.cb.is_null() {
            unsafe {
                trigger_cb();
            }
        }
    }
}

impl Drop for Closure {
    fn drop(&mut self) {
        self.dealloc_cb();
    }
}

fn main() {
    let mut closure = Closure::new();
    closure.register(|| {
        println!("Hello from Rust");
    });
    closure.trigger();
}
