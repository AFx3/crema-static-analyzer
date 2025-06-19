use std::{
    ptr,
    sync::atomic::{AtomicPtr, Ordering},
    thread::spawn,
};

struct Node<T> {
    data: T,
    next: *mut Node<T>,
}
struct LockFreeStack<T> {
    head: AtomicPtr<Node<T>>,
}

unsafe impl<T> Sync for LockFreeStack<T> where T: Send {}

impl<T> LockFreeStack<T> {
    fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    fn push(&self, data: T) {
        let new_node = Box::new(Node {
            data,
            next: ptr::null_mut(),
        });
        let new_node_ptr = Box::into_raw(new_node);

        loop {
            // atomicly get a pointer to node pointed by head
            let current = self.head.load(Ordering::SeqCst);
            // set new nodes next to the node pointed by head currently
            unsafe {
                (*new_node_ptr).next = current;
            }
            // If current and head are still pointing to the same node then exchange head with the pointer pointing to the new node
            if self
                .head
                .compare_exchange_weak(current, new_node_ptr, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
            // ElSE, a thread has disturbed the operation between load and exchange, we must retry
        }
    }

    fn pop(&self) -> Option<T> {
        loop {
            let current_head = self.head.load(Ordering::SeqCst);
            if current_head.is_null() {
                return None;
            }
            let next = (unsafe { current_head.read() }).next; // readvolatile() has nothing to do with atmoics
            
            // If head has not changed since load, point head to next node
            if self
                .head
                .compare_exchange_weak(current_head, next, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                // Now we own the node and can safely deallocate it
                let node = unsafe { Box::from_raw(current_head) };
                return Some(node.data);
            }
        }
    }

    /// This is not safe for concurrent use, it is only for debugging
    fn len(&self) -> u64 {
        let mut current = self.head.load(Ordering::SeqCst);
        let mut count = 0_u64;
        while !current.is_null() {
            count += 1;
            current = (unsafe { current.read_volatile() }).next
        }
        count
    }
}

fn main() {
    let stack: &'static _ = Box::leak(Box::new(LockFreeStack::new()));
        let handles: Vec<_> = (0..10)
            .map(|i| {
                spawn(move || {
                    for _ in 0..1000 {
                        stack.push(i);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        println!("len: {}",stack.len());
        println!("top element: {:?}", stack.pop());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push() {
        let stack: &'static _ = Box::leak(Box::new(LockFreeStack::new()));
        let handles: Vec<_> = (0..10)
            .map(|i| {
                spawn(move || {
                    for _ in 0..100000 {
                        stack.push(i);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(stack.len(), 1_000_000)
    }

    #[test]
    fn test_pop() {
        let stack: &'static _ = Box::leak(Box::new(LockFreeStack::new()));
        for i in 0..100000 {
            stack.push(i);
        }
        let handles: Vec<_> = (0..10)
            .map(|_| {
                spawn(move || {
                    for _ in 0..100000 {
                        let _ = stack.pop();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(stack.len(), 0)
    }
}
