use std::fmt::Debug;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::Arc;
use std::thread;

pub struct Node<T> {
    pub data: T,
    pub next: AtomicPtr<Node<T>>,
}

pub struct LockFreeLinkedList<T: Debug> {
    pub head: AtomicPtr<Node<T>>,
}

impl<T: Debug> LockFreeLinkedList<T> {
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    pub fn insert(&self, data: T) {
        let new_node = Box::into_raw(Box::new(Node {
            data,
            next: AtomicPtr::new(ptr::null_mut()),
        }));

        loop {
            let head = self.head.load(Ordering::Acquire);
            unsafe {
                (*new_node).next.store(head, Ordering::Relaxed);
            }
            if self
                .head
                .compare_exchange(head, new_node, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                break;
            }
        }
    }

    pub fn traverse(&self) {
        let mut current = self.head.load(Ordering::Acquire);
        while !current.is_null() {
            unsafe {
                println!("{:?}", (*current).data);
                current = (*current).next.load(Ordering::Acquire);
            }
        }
    }
}

impl<T: Debug> Drop for LockFreeLinkedList<T> {
    fn drop(&mut self) {
        let mut current = self.head.load(Ordering::Relaxed);
        while !current.is_null() {
            unsafe {
                let next = (*current).next.load(Ordering::Relaxed);
                let _ = Box::from_raw(current);
                current = next;
            }
        }
    }
}

fn main() {
    let list = Arc::new(LockFreeLinkedList::new());

    // Insert elements in a single-threaded manner
    list.insert(1);
    list.insert(2);
    list.insert(3);

    // Print the list
    println!("Single-threaded insertion:");
    list.traverse();

    // Multi-threaded insertion example
    let list = Arc::new(LockFreeLinkedList::new());
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let list = Arc::clone(&list);
            thread::spawn(move || {
                list.insert(i);
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Print the list after multi-threaded insertion
    println!("Multi-threaded insertion:");
    list.traverse();
}
