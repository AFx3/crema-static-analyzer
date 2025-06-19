use std::collections::HashMap;

macro_rules! hash_map {
    ($($key: expr => $val: expr), *) => {
        {
            let mut map = HashMap::new();
            $(
                map.insert($key, $val);
            )*
            map
        }
    };
}

use std::cell::RefCell;
use std::ops::Deref;
use std::ptr::NonNull;

struct SimpleStack<T> {
    stack: RefCell<Vec<T>>,
}

impl<T> SimpleStack<T> {
    fn new() -> SimpleStack<T> {
        SimpleStack {
            stack: RefCell::new(Vec::new()),
        }
    }

    fn push(&self, item: T) {
        self.stack.borrow_mut().push(item);
    }

    fn pop(&self) -> Option<T> {
        self.stack.borrow_mut().pop()
    }
}

struct MyBox<T> {
    value: T,
    ref_count: RefCell<usize>,
}
struct MyRc<T> {
    ptr: NonNull<MyBox<T>>,
}

impl<T> MyRc<T> {
    fn new(value: T) -> MyRc<T> {
        let boxed = Box::new(MyBox {
            value,
            ref_count: RefCell::new(1),
        });

        MyRc {
            ptr: unsafe { NonNull::new_unchecked(Box::into_raw(boxed)) },
        }
    }
}

impl<T> Clone for MyRc<T> {
    fn clone(&self) -> MyRc<T> {
        unsafe {
            *((*self.ptr.as_ptr()).ref_count.borrow_mut()) += 1;
        }

        MyRc { ptr: self.ptr }
    }
}

impl<T> Drop for MyRc<T> {
    fn drop(&mut self) {
        unsafe {
            if *((*self.ptr.as_ptr()).ref_count.borrow_mut()) == 1 {
                drop(Box::from_raw(self.ptr.as_ptr()));
            } else {
                *((*self.ptr.as_ptr()).ref_count.borrow_mut()) -= 1;
            }
        }
    }
}

impl<T> Deref for MyRc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &unsafe { &*self.ptr.as_ptr() }.value
    }
}

fn main() {
    println!("Hello, world!");

    let map = hash_map!(
        "one" => 1,
        "two" => 2,
        "three" => 3
    );

    println!("{:?}", map);

    let stack = SimpleStack::new();
    stack.push(1);
    stack.push(2);
    stack.push(3);

    println!("{:?}", stack.pop());
    println!("{:?}", stack.pop());

    stack.push(4);

    println!("{:?}", stack.pop());
    println!("{:?}", stack.pop());
    println!("{:?}", stack.pop());

    {
        let rc = MyRc::new(1);

        {
            let rc2 = rc.clone();
            println!("{}", *rc2);
        }

        println!("{}", *rc);
    }

    

}
