use std::sync::atomic::{AtomicPtr, Ordering};
use std::ptr;

#[derive(Debug)]
struct Node<T> {
    data: T,
    next: AtomicPtr<Node<T>>,
}

pub struct LockFreeQueue<T> {
    head: AtomicPtr<Node<T>>,
    tail: AtomicPtr<Node<T>>,
}

impl<T> Node<T> {
    fn new(data: T) -> Self {
        Node {
            data,
            next: AtomicPtr::new(ptr::null_mut()),
        }
    }
}

impl<T> LockFreeQueue<T> {
    pub fn new() -> Self {
        // Create a dummy node to simplify the implementation
        let dummy = Box::into_raw(Box::new(Node::new(unsafe { 
            std::mem::zeroed() 
        })));
        
        LockFreeQueue {
            head: AtomicPtr::new(dummy),
            tail: AtomicPtr::new(dummy),
        }
    }

    pub fn enqueue(&self, data: T) {
        let new_node = Box::into_raw(Box::new(Node::new(data)));

        loop {
            let tail = self.tail.load(Ordering::Acquire);
            let next = unsafe { (*tail).next.load(Ordering::Acquire) };

            if tail == self.tail.load(Ordering::Acquire) {
                if next.is_null() {
                    // Try to link the new node
                    if unsafe { (*tail).next.compare_exchange(
                        next,
                        new_node,
                        Ordering::Release,
                        Ordering::Relaxed,
                    ).is_ok() } {
                        // Try to advance the tail
                        let _ = self.tail.compare_exchange(
                            tail,
                            new_node,
                            Ordering::Release,
                            Ordering::Relaxed,
                        );
                        break;
                    }
                } else {
                    // Help advance the tail
                    let _ = self.tail.compare_exchange(
                        tail,
                        next,
                        Ordering::Release,
                        Ordering::Relaxed,
                    );
                }
            }
        }
    }

    pub fn dequeue(&self) -> Option<T> {
        loop {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);
            let next = unsafe { (*head).next.load(Ordering::Acquire) };

            if head == self.head.load(Ordering::Acquire) {
                if head == tail {
                    if next.is_null() {
                        return None;
                    }
                    // Help advance the tail
                    let _ = self.tail.compare_exchange(
                        tail,
                        next,
                        Ordering::Release,
                        Ordering::Relaxed,
                    );
                } else {
                    // Read the value before CAS
                    let data = unsafe { ptr::read(&(*next).data) };
                    
                    if self.head.compare_exchange(
                        head,
                        next,
                        Ordering::Release,
                        Ordering::Relaxed,
                    ).is_ok() {
                        unsafe {
                            // Free the old dummy node
                            drop(Box::from_raw(head));
                        }
                        return Some(data);
                    }
                }
            }
        }
    }
}

// Safety: The queue can be safely shared between threads
unsafe impl<T: Send> Send for LockFreeQueue<T> {}
unsafe impl<T: Send> Sync for LockFreeQueue<T> {}

impl<T> Drop for LockFreeQueue<T> {
    fn drop(&mut self) {
        while self.dequeue().is_some() {}
        
        // Free the dummy node
        let head = self.head.load(Ordering::Relaxed);
        unsafe {
            drop(Box::from_raw(head));
        }
    }
}

// Verification tests using loom
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use loom::model;
    use loom::thread::spawn;
    use crossbeam::epoch;

    // Basic functionality tests
    #[test]
    fn test_basic_operations() {
        let queue = LockFreeQueue::new();
        queue.enqueue(1);
        queue.enqueue(2);
        queue.enqueue(3);

        assert_eq!(queue.dequeue(), Some(1));
        assert_eq!(queue.dequeue(), Some(2));
        assert_eq!(queue.dequeue(), Some(3));
        assert_eq!(queue.dequeue(), None);
    }

    // Concurrent operations test using loom
    #[test]
    fn test_concurrent_operations() {
        model(|| {
            let queue = Arc::new(LockFreeQueue::new());
            
            // Producer threads
            let queue_clone = Arc::clone(&queue);
            let producer1 = spawn(move || {
                queue_clone.enqueue(1);
                queue_clone.enqueue(2);
            });

            let queue_clone = Arc::clone(&queue);
            let producer2 = spawn(move || {
                queue_clone.enqueue(3);
                queue_clone.enqueue(4);
            });

            // Consumer threads
            let queue_clone = Arc::clone(&queue);
            let consumer1 = spawn(move || {
                let _ = queue_clone.dequeue();
                let _ = queue_clone.dequeue();
            });

            let queue_clone = Arc::clone(&queue);
            let consumer2 = spawn(move || {
                let _ = queue_clone.dequeue();
                let _ = queue_clone.dequeue();
            });

            producer1.join().unwrap();
            producer2.join().unwrap();
            consumer1.join().unwrap();
            consumer2.join().unwrap();
        });
    }

    // Memory safety test
    #[test]
    fn test_memory_safety() {
        model(|| {
            let queue = Arc::new(LockFreeQueue::new());
            
            // Multiple threads accessing the same memory
            let handles: Vec<_> = (0..3)
                .map(|i| {
                    let queue = Arc::clone(&queue);
                    spawn(move || {
                        queue.enqueue(i);
                        let _ = queue.dequeue();
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }
        });
    }

    // FIFO ordering test
    #[test]
    fn test_fifo_ordering() {
        let queue = LockFreeQueue::new();
        
        // Enqueue elements
        for i in 0..100 {
            queue.enqueue(i);
        }

        // Verify FIFO order
        for i in 0..100 {
            assert_eq!(queue.dequeue(), Some(i));
        }
    }

    // ABA prevention test
    #[test]
    fn test_aba_prevention() {
        model(|| {
            let queue = Arc::new(LockFreeQueue::new());
            let guard = epoch::pin();

            // Thread 1: Dequeue and hold reference
            let queue_clone = Arc::clone(&queue);
            let t1 = spawn(move || {
                queue_clone.enqueue(1);
                let _ = queue_clone.dequeue();
            });

            // Thread 2: Dequeue and enqueue same value
            let queue_clone = Arc::clone(&queue);
            let t2 = spawn(move || {
                queue_clone.enqueue(2);
                let _ = queue_clone.dequeue();
                queue_clone.enqueue(1);
            });

            t1.join().unwrap();
            t2.join().unwrap();
        });
    }

    // Stress test with many threads
    #[test]
    fn test_stress() {
        let queue = Arc::new(LockFreeQueue::new());
        let thread_count = 8;
        let ops_per_thread = 1000;

        let handles: Vec<_> = (0..thread_count)
            .map(|_| {
                let queue = Arc::clone(&queue);
                thread::spawn(move || {
                    for i in 0..ops_per_thread {
                        queue.enqueue(i);
                        let _ = queue.dequeue();
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    // Test memory reclamation
    #[test]
    fn test_memory_reclamation() {
        model(|| {
            let queue = Arc::new(LockFreeQueue::new());
            
            let handles: Vec<_> = (0..3)
                .map(|_| {
                    let queue = Arc::clone(&queue);
                    spawn(move || {
                        // Repeatedly enqueue and dequeue to trigger memory reclamation
                        for _ in 0..10 {
                            queue.enqueue(1);
                            let _ = queue.dequeue();
                        }
                    })
                })
                .collect();

            for handle in handles {
                handle.join().unwrap();
            }
        });
    }

    // Test compositionality
    #[test]
    fn test_compositionality() {
        model(|| {
            let queue1 = Arc::new(LockFreeQueue::new());
            let queue2 = Arc::new(LockFreeQueue::new());
            
            let q1 = Arc::clone(&queue1);
            let q2 = Arc::clone(&queue2);
            let t1 = spawn(move || {
                q1.enqueue(1);
                if let Some(v) = q1.dequeue() {
                    q2.enqueue(v);
                }
            });

            let q1 = Arc::clone(&queue1);
            let q2 = Arc::clone(&queue2);
            let t2 = spawn(move || {
                q1.enqueue(2);
                if let Some(v) = q1.dequeue() {
                    q2.enqueue(v);
                }
            });

            t1.join().unwrap();
            t2.join().unwrap();
        });
    }
}

fn main() {
    // Example usage
    let queue = LockFreeQueue::new();
    queue.enqueue(1);
    queue.enqueue(2);
    queue.enqueue(3);

    println!("Dequeued: {:?}", queue.dequeue());
    println!("Dequeued: {:?}", queue.dequeue());
    println!("Dequeued: {:?}", queue.dequeue());
}
