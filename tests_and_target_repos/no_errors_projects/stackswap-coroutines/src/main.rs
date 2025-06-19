use std::collections::HashMap;
use std::ffi::c_void;
use std::mem::MaybeUninit;

use setjmp::*;

type YieldValue = u128;

pub struct Dispatcher {
    jmpbuf: *mut jmp_buf,
    allocator: Box<dyn StackBufferAllocator>,
    stacks: Vec<Stack>,
    current_stack: usize,
    retrieved_values: HashMap<u64, YieldValue>,
}

impl Dispatcher {
    pub fn new(allocator: Box<dyn StackBufferAllocator>) -> Self {
        Self {
            #[allow(dangling_pointers_from_temporaries)]
            jmpbuf: MaybeUninit::uninit().as_mut_ptr(),
            allocator,
            stacks: Vec::new(),
            current_stack: 0,
            retrieved_values: HashMap::new(),
        }
    }

    pub fn abort(&self) {
        unsafe {
            longjmp(self.jmpbuf, 1);
        }
    }

    #[inline(never)]
    pub fn run(&mut self) {
        let mut jmpbuf: MaybeUninit<jmp_buf> = MaybeUninit::uninit();
        unsafe {
            setjmp(jmpbuf.as_mut_ptr());
        }
        self.jmpbuf = jmpbuf.as_mut_ptr();

        // while we have a stack to use
        while let Some(stack) = self.next() {
            if stack.is_complete {
                self.stacks.remove(self.current_stack);
                if self.current_stack >= self.stacks.len() && !self.stacks.is_empty() {
                    self.current_stack = 0;
                }
                continue;
            }

            // continue/launch the stack
            if let Some(_) = stack.bottom {
                stack.cont();
            } else {
                stack.launch();
            }
        }
    }

    fn next(&mut self) -> Option<&mut Stack> {
        if self.stacks.is_empty() {
            return None;
        }

        // Try each stack once, starting from current_stack
        let start = self.current_stack;
        loop {
            let current = self.current_stack;
            self.current_stack = (self.current_stack + 1) % self.stacks.len();

            let stack = &self.stacks[current];
            let (waiting_on, current_id) = (stack.waiting_on, stack.id);

            // If stack isn't waiting, it can run
            if waiting_on.is_none() {
                return Some(&mut self.stacks[current]);
            }

            let child_id = waiting_on.unwrap();

            // Check if child has yielded a value
            if let Some(child_idx) = self.stacks.iter().position(|s| s.id == child_id) {
                if let Some(value) = self.stacks[child_idx].value.take() {
                    // Store with waiting stack's ID (current_id)
                    self.retrieved_values.insert(current_id, value);
                    return Some(&mut self.stacks[current]);
                }
            }

            // Check if child is complete
            let child_exists = self
                .stacks
                .iter()
                .any(|s| s.id == child_id && !s.is_complete);
            if !child_exists {
                let stack = &mut self.stacks[current];
                stack.waiting_on = None;
                return Some(stack);
            }

            // If we've tried all stacks and found nothing runnable, return None
            if self.current_stack == start {
                return None;
            }
        }
    }

    fn take_retrieved_value(&mut self, stack_id: u64) -> Option<YieldValue> {
        // Get value using the requesting stack's ID
        self.retrieved_values.remove(&stack_id)
    }

    fn next_id(&self) -> u64 {
        let max_id = self.stacks.iter().map(|stack| stack.id).max().unwrap_or(0);
        max_id + 1
    }
}

pub trait StackBufferAllocator {
    fn alloc(&self, len: usize) -> StackBuffer;
    fn free(&self, stack_buffer: StackBuffer);
}

#[derive(Debug)]
pub struct StackBuffer {
    /// location
    loc: *mut c_void,
    /// length
    len: usize,
}

pub struct Stack {
    id: u64,
    dispatcher: *mut Dispatcher,
    pub bottom: Option<*mut c_void>,
    top: *mut c_void,
    stack_buffer: Option<StackBuffer>,
    continue_jmpbuf: *mut jmp_buf,
    program: Option<Box<dyn FnOnce(&mut Stack)>>,
    is_complete: bool,
    value: Option<YieldValue>,
    waiting_on: Option<u64>,
}

impl Stack {
    pub fn new(id: u64, dispatcher: *mut Dispatcher, program: Box<dyn FnOnce(&mut Stack)>) -> Self {
        assert!(!dispatcher.is_null(), "Null dispatcher in Stack::new");
        Self {
            id,
            dispatcher,
            bottom: None,
            top: std::ptr::null_mut(),
            stack_buffer: None,
            continue_jmpbuf: std::ptr::null_mut(),
            program: Some(program),
            is_complete: false,
            value: None,
            waiting_on: None,
        }
    }

    #[inline(never)]
    pub fn stack_in(&mut self) {
        if let Some(stack_buffer) = &self.stack_buffer {
            let bottom = self.bottom.expect("No bottom pointer set in stack_in");
            let used = bottom.addr() - self.top.addr();

            assert!(used > 0, "Invalid stack size in stack_in");
            assert!(used <= stack_buffer.len, "Buffer overflow in stack_in");

            unsafe {
                std::ptr::copy_nonoverlapping(stack_buffer.loc, self.top, used);
            }
        } else {
            panic!("stack_in: no stack buffer for stack {}", self.id);
        }
    }

    #[inline(never)]
    pub fn stack_out(&mut self) {
        assert!(!self.dispatcher.is_null(), "Dispatcher null in stack_out");
        let bottom = self.bottom.expect("No bottom pointer set");
        let used = bottom.addr() - self.top.addr();
        assert!(used > 0, "Stack underflow detected");

        // Add more padding to buffer
        let required_size = used + 64; // More padding

        // Ensure we have enough buffer space
        if self
            .stack_buffer
            .as_ref()
            .map_or(true, |buf| buf.len < required_size)
        {
            if let Some(old_buf) = self.stack_buffer.take() {
                unsafe {
                    assert!(!self.dispatcher.is_null(), "Dispatcher null before free");
                    (*self.dispatcher).allocator.free(old_buf);
                }
            }

            self.stack_buffer = Some(unsafe {
                assert!(!self.dispatcher.is_null(), "Dispatcher null before alloc");
                (*self.dispatcher).allocator.alloc(required_size)
            });
        }

        unsafe {
            let buffer = self.stack_buffer.as_ref().unwrap();
            assert!(buffer.len >= used, "Buffer too small for stack");
            std::ptr::copy_nonoverlapping(self.top, buffer.loc, used);
        }
    }

    #[inline(never)]
    pub fn cont(&mut self) {
        if self.is_complete {
            eprintln!(
                "Warning: Attempting to continue completed stack {}",
                self.id
            );
            return;
        }
        self.stack_in();
        unsafe {
            longjmp(self.continue_jmpbuf, 1);
        }
    }

    #[inline(never)]
    pub fn launch(&mut self) {
        let mut padding = [0u8; 512];
        self.bottom = Some(padding.as_mut_ptr() as *mut c_void);
        if let Some(program) = self.program.take() {
            program(self);
            self.bottom = None;
            self.is_complete = true;

            // Make sure we return to dispatcher after completion
            unsafe {
                assert!(
                    !(*self.dispatcher).jmpbuf.is_null(),
                    "Invalid dispatcher jmpbuf"
                );
                longjmp((*self.dispatcher).jmpbuf, 1);
            }
        }
    }

    #[inline(never)]
    pub fn block(&mut self) {
        unsafe {
            let mut jmpbuf: MaybeUninit<jmp_buf> = MaybeUninit::uninit();
            if setjmp(jmpbuf.as_mut_ptr()) == 0 {
                self.continue_jmpbuf = jmpbuf.as_mut_ptr();

                const STACK_PADDING: usize = 64;
                let frame_addr = self.continue_jmpbuf.addr();
                assert!(frame_addr >= STACK_PADDING, "Stack frame address too low");

                // Calculate aligned top with padding
                let unaligned_top = frame_addr - STACK_PADDING;
                let aligned_top = (unaligned_top + 15) & !15;
                self.top = (aligned_top as *mut c_void).cast();

                // Additional safety checks
                if let Some(bottom) = self.bottom {
                    let available_space = bottom.addr() - self.top.addr();
                    assert!(
                        available_space >= STACK_PADDING,
                        "Insufficient stack space: {} bytes",
                        available_space
                    );
                    assert!(
                        bottom.addr() > self.top.addr(),
                        "Stack overflow detected: bottom={:p} top={:p}",
                        bottom,
                        self.top
                    );
                }

                assert!(
                    self.top.addr() % 16 == 0,
                    "Stack misaligned: {:p}",
                    self.top
                );

                self.stack_out();

                // Verify dispatcher pointer before jump
                assert!(
                    !(*self.dispatcher).jmpbuf.is_null(),
                    "Invalid dispatcher jmpbuf before longjmp"
                );

                longjmp((*self.dispatcher).jmpbuf, 1);
            }
        }
    }

    pub fn yield_value(&mut self, value: YieldValue) {
        self.value = Some(value);
        self.block();
    }

    pub fn get_yielded(&mut self, child_id: u64) -> Option<YieldValue> {
        assert_ne!(self.id, child_id, "Stack {} is waiting on itself", self.id);
        println!(
            "get_yielded: child_id={}, waiting_on={:?}",
            child_id, self.waiting_on
        );
        assert!(
            !self.dispatcher.is_null(),
            "Dispatcher is null, the stack is broken"
        );
        unsafe {
            // First check if we already have a value
            if let Some(value) = (*self.dispatcher).take_retrieved_value(self.id) {
                println!("Got value from dispatcher: {}", value);
                return Some(value);
            }

            // No value yet, set waiting and block
            println!("Setting waiting_on for stack {} to {}", self.id, child_id);
            self.waiting_on = Some(child_id);

            // Keep blocking until we either get a value or child completes
            loop {
                println!("Blocking stack {}", self.id);
                self.block();

                // After each block, check for value first
                println!("Checking for value after block");
                if let Some(value) = (*self.dispatcher).take_retrieved_value(self.id) {
                    self.waiting_on = None;
                    return Some(value);
                }

                // Then check if child still exists
                let child_exists = (*self.dispatcher)
                    .stacks
                    .iter()
                    .any(|s| s.id == child_id && !s.is_complete);

                if !child_exists {
                    self.waiting_on = None;
                    return None;
                }

                // Still waiting for child
            }
        }
    }

    pub fn spawn<F>(&mut self, f: F) -> u64
    where
        F: FnOnce(&mut Stack) + 'static,
    {
        unsafe {
            let new_id = (*self.dispatcher).next_id();
            let new_stack = Stack::new(new_id, self.dispatcher, Box::new(f));
            (*self.dispatcher).stacks.push(new_stack);
            new_id
        }
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        if let Some(buffer) = self.stack_buffer.take() {
            unsafe {
                (*self.dispatcher).allocator.free(buffer);
            }
        }
    }
}

struct HeapAllocator {
    allocations: std::sync::atomic::AtomicUsize,
}

impl HeapAllocator {
    fn new() -> Self {
        Self {
            allocations: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl StackBufferAllocator for HeapAllocator {
    fn alloc(&self, len: usize) -> StackBuffer {
        // Ensure minimum allocation size
        let min_size = 4096;
        let len = std::cmp::max(len, min_size);

        // Align to 16 bytes
        let len = (len + 15) & !15;

        // Use Layout for proper alignment
        let layout = std::alloc::Layout::from_size_align(len, 16).expect("Failed to create layout");

        let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }

        self.allocations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        StackBuffer {
            loc: ptr as *mut c_void,
            len,
        }
    }

    fn free(&self, stack_buffer: StackBuffer) {
        let current = self.allocations.load(std::sync::atomic::Ordering::SeqCst);
        if current == 0 {
            eprintln!("Warning: Attempting to free buffer when allocation count is 0");
            return;
        }

        self.allocations
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

        let len = (stack_buffer.len + 15) & !15;
        unsafe {
            let layout =
                std::alloc::Layout::from_size_align(len, 16).expect("Failed to create layout");
            std::alloc::dealloc(stack_buffer.loc as *mut u8, layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number_sequence(stack: &mut Stack) {
        println!("Starting number_sequence");
        for i in 0..5 {
            println!("Yielding value: {}", i);
            stack.yield_value(i as YieldValue);
            println!("After yield: {}", i);
        }
        println!("Ending number_sequence");
    }

    fn parent_routine(stack: &mut Stack) {
        println!("Starting parent_routine");
        let child_id = stack.spawn(number_sequence);
        println!("Spawned child with id: {}", child_id);

        while let Some(value) = stack.get_yielded(child_id) {
            println!("Parent received value: {}", value);
        }
        println!("Parent ending");
    }

    #[test]
    fn test_parent_child_sequence() {
        println!("Launching Dispatcher.");
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(0, d, Box::new(parent_routine));
        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_simple_yield_sequence() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                for i in 0..5 {
                    stack.yield_value(i as YieldValue);
                }
            }),
        );
        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_parent_child_communication() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                let child_id = stack.spawn(|child| {
                    for i in 0..5 {
                        child.yield_value((i * 2) as YieldValue);
                    }
                });

                let mut sum = 0;
                while let Some(value) = stack.get_yielded(child_id) {
                    sum += value;
                }
                assert_eq!(sum, 20); // 0 + 2 + 4 + 6 + 8
            }),
        );
        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_multiple_children() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                // Spawn two children
                let child1_id = stack.spawn(|child| {
                    for i in 0..3 {
                        child.yield_value(i as YieldValue);
                    }
                });

                let child2_id = stack.spawn(|child| {
                    for i in 0..3 {
                        child.yield_value((i + 10) as YieldValue);
                    }
                });

                // Collect values from both children alternately
                let mut values1 = Vec::new();
                let mut values2 = Vec::new();
                let mut done1 = false;
                let mut done2 = false;

                while !done1 || !done2 {
                    if !done1 {
                        if let Some(value) = stack.get_yielded(child1_id) {
                            values1.push(value);
                        } else {
                            done1 = true;
                        }
                    }
                    if !done2 {
                        if let Some(value) = stack.get_yielded(child2_id) {
                            values2.push(value);
                        } else {
                            done2 = true;
                        }
                    }
                }

                assert_eq!(values1, vec![0, 1, 2]);
                assert_eq!(values2, vec![10, 11, 12]);
            }),
        );
        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_nested_coroutines() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                let child_id = stack.spawn(|child| {
                    // Spawn a grandchild from the child
                    let grandchild_id = child.spawn(|grandchild| {
                        for i in 0..3 {
                            grandchild.yield_value(i as YieldValue);
                        }
                    });

                    // Child modifies and forwards values from grandchild
                    while let Some(value) = child.get_yielded(grandchild_id) {
                        child.yield_value(value * 2);
                    }
                });

                let mut values = Vec::new();
                while let Some(value) = stack.get_yielded(child_id) {
                    values.push(value);
                }
                assert_eq!(values, vec![0, 2, 4]);
            }),
        );
        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_stack_growth() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                let child_id = stack.spawn(|s| {
                    // Function to measure current stack size and print details
                    fn get_stack_info(s: &Stack) -> (usize, String) {
                        if let Some(bottom) = s.bottom {
                            let stack_marker = 0u8; // Local variable to get current stack position
                            let current_pos = &stack_marker as *const u8 as *mut c_void;
                            let size = bottom.addr() - current_pos.addr();
                            let info = format!(
                                "bottom: {:p}, current: {:p}, size: {} bytes",
                                bottom, current_pos, size
                            );
                            (size, info)
                        } else {
                            (0, "no bottom pointer".to_string())
                        }
                    }

                    // Create progressively larger stack allocations
                    for i in 0..4 {
                        // Print stack info before allocation
                        let (initial_size, initial_info) = get_stack_info(s);
                        println!("Before allocation {}: {}", i, initial_info);

                        // Recursive function to create stack frames and measure stack
                        fn recursive_stack_growth(
                            s: &Stack,
                            depth: usize,
                            target: usize,
                            array: &mut [u8; 256],
                        ) -> usize {
                            // Measure current stack size at this depth
                            let (current_size, info) = get_stack_info(s);
                            println!("  At depth {}: {}", depth, info);

                            if depth < target {
                                // Modify array to prevent optimization
                                for i in 0..array.len() {
                                    array[i] = array[i].wrapping_add(1);
                                }
                                // Recurse and return deepest stack size
                                recursive_stack_growth(s, depth + 1, target, array)
                            } else {
                                current_size // Return stack size at maximum depth
                            }
                        }

                        // Create stack growth through recursion
                        let mut array = [0u8; 256];
                        let depth = 1 << i; // 1, 2, 4, 8 stack frames
                        let max_stack_size = recursive_stack_growth(s, 0, depth, &mut array);

                        // Print stack info after allocation
                        let (_final_size, final_info) = get_stack_info(s);
                        println!("After allocation {}: {}", i, final_info);
                        println!(
                            "Created {} stack frames, max stack size: {} bytes (grew by: {} bytes)",
                            depth,
                            max_stack_size,
                            max_stack_size.saturating_sub(initial_size)
                        );

                        // Yield maximum stack size seen
                        s.yield_value(max_stack_size as YieldValue);
                    }
                });

                // Collect and verify the yielded values
                let mut sizes = Vec::new();
                while let Some(size) = stack.get_yielded(child_id) {
                    println!("\nStack measurement: {} bytes", size);
                    sizes.push(size);
                }

                // Print all collected sizes
                println!("\nAll stack sizes: {:?}", sizes);

                // Verify we got 4 measurements
                assert_eq!(sizes.len(), 4, "Should have 4 stack measurements");
            }),
        );

        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    fn test_coroutine(stack: &mut Stack, counter: *mut u64) {
        let mut runs = 0u64;
        let mut local = 0u64;
        let mut growing_vec = Vec::new();

        unsafe {
            loop {
                if *counter >= 1_000_000 {
                    *counter += 1;
                    if *counter % 1_000 == 0 {
                        eprintln!(
                            "stack {} | local:{} counter:{} vec_size:{} (DONE)",
                            stack.id,
                            local,
                            *counter,
                            growing_vec.len()
                        );
                    }
                    return;
                } else if *counter % 10_000 == 0 {
                    eprintln!(
                        "stack {} | local:{} counter:{} vec_size:{}",
                        stack.id,
                        local,
                        *counter,
                        growing_vec.len()
                    );
                }

                growing_vec.push(runs);
                local += *counter / 2;
                *counter += 1;
                runs += 1;
                stack.block();
            }
        }
    }

    #[test]
    fn test_counter_coroutine() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let counter = Box::into_raw(Box::new(0u64));

        // Create multiple coroutines all sharing the counter
        let num_coroutines = 4_000;
        for i in 0..num_coroutines {
            let counter_ptr = counter; // Share the same counter pointer
            let stack = Stack::new(i, d, Box::new(move |s| test_coroutine(s, counter_ptr)));
            dispatcher.stacks.push(stack);
        }

        dispatcher.run();

        unsafe {
            let final_count = *counter;
            drop(Box::from_raw(counter));
            assert!(
                final_count >= 1_000_000,
                "Counter should reach at least 1,000,000"
            );
        }
    }

    #[test]
    fn test_massive_spawn_chain() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        // Just try two levels
        const MAX_DEPTH: u64 = 2;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                fn spawn_chain(stack: &mut Stack, depth: u64, max_depth: u64) {
                    if depth < max_depth {
                        // Spawn child and get its ID
                        let child_id = stack.spawn(move |child| {
                            // Just yield and return immediately
                            child.yield_value(1_u128);
                        });

                        // Wait for child to complete
                        while let Some(_) = stack.get_yielded(child_id) {}
                    }
                }

                spawn_chain(stack, 0, MAX_DEPTH);
            }),
        );

        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    // #[test]
    // fn test_concurrent_memory_stress() {
    //     let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
    //     let d = &mut dispatcher as *mut Dispatcher;

    //     // Spawn 100 coroutines that each allocate and deallocate memory
    //     let stack = Stack::new(0, d, Box::new(|stack| {
    //         let mut child_ids = Vec::new();

    //         // Spawn memory-intensive children
    //         for i in 0..100 {
    //             let child_id = stack.spawn(move |child| {
    //                 let mut vectors = Vec::new();
    //                 for j in 0..100 {
    //                     // Allocate a vector with increasing size
    //                     let mut vec = Vec::with_capacity(1000 * (j + 1));
    //                     vec.extend((0..vec.capacity()).map(|x| x as u8));
    //                     vectors.push(vec);

    //                     // Yield after each allocation
    //                     child.yield_value((i * 1000 + j) as YieldValue);
    //                 }
    //                 // Let vectors drop here
    //             });
    //             child_ids.push(child_id);
    //         }

    //         // Wait for all children to complete
    //         for child_id in child_ids {
    //             while let Some(_) = stack.get_yielded(child_id) {}
    //         }
    //     }));

    //     dispatcher.stacks.push(stack);
    //     dispatcher.run();
    // }

    #[test]
    fn test_ping_pong_stress() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        // Create two coroutines that rapidly yield values back and forth
        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                let pong_id = stack.spawn(move |pong| {
                    let mut count = 0;
                    while count < 1_000_000 {
                        pong.yield_value(count);
                        count += 1;
                    }
                });

                let ping_id = stack.spawn(move |ping| {
                    while let Some(value) = ping.get_yielded(pong_id) {
                        ping.yield_value(value + 1);
                    }
                });

                // Monitor both coroutines
                let mut ping_done = false;
                let mut pong_done = false;
                let mut last_value = 0;

                while !ping_done || !pong_done {
                    if !ping_done {
                        if let Some(value) = stack.get_yielded(ping_id) {
                            assert!(value > last_value, "Values should be strictly increasing");
                            last_value = value;
                        } else {
                            ping_done = true;
                        }
                    }
                    if !pong_done {
                        if let Some(value) = stack.get_yielded(pong_id) {
                            assert!(value > last_value, "Values should be strictly increasing");
                            last_value = value;
                        } else {
                            pong_done = true;
                        }
                    }
                }
            }),
        );

        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_recursive_fibonacci() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                // Compute fibonacci using coroutines that yield intermediate values
                fn fib(stack: &mut Stack, n: u64) -> YieldValue {
                    println!("Computing fib({}) on stack {}", n, stack.id);

                    // Base cases
                    if n <= 1 {
                        println!("Base case: fib({}) = {}", n, n);
                        return n as YieldValue;
                    }

                    // Spawn child coroutine for n-1
                    println!("Spawning child for fib({})", n - 1);
                    let child1_id = stack.spawn(move |child| {
                        let result = fib(child, n - 1);
                        child.yield_value(result);
                    });

                    // Spawn child coroutine for n-2
                    println!("Spawning child for fib({})", n - 2);
                    let child2_id = stack.spawn(move |child| {
                        let result = fib(child, n - 2);
                        child.yield_value(result);
                    });

                    // Get results from both children
                    let mut n1 = 0;
                    let mut n2 = 0;

                    if let Some(value) = stack.get_yielded(child1_id) {
                        println!("Received n1 = {} for fib({})", value, n);
                        n1 = value;
                    }

                    if let Some(value) = stack.get_yielded(child2_id) {
                        println!("Received n2 = {} for fib({})", value, n);
                        n2 = value;
                    }

                    n1 + n2
                }

                // Start the fibonacci calculation with a smaller number
                let result = fib(stack, 6);
                stack.yield_value(result);
            }),
        );

        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_stack_intensive_coroutines() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                // Simple function that yields a few times
                fn simple_work(stack: &mut Stack, id: usize) {
                    println!("Starting work for coroutine {}", id);

                    // First yield
                    stack.yield_value(id as YieldValue);
                    println!("Coroutine {} after first yield", id);

                    // Block and yield again
                    stack.block();
                    stack.yield_value((id + 100) as YieldValue);
                    println!("Coroutine {} after second yield", id);

                    // One more time
                    stack.block();
                    stack.yield_value((id + 200) as YieldValue);
                    println!("Coroutine {} completing", id);
                }

                // Just spawn 2 coroutines initially to test
                println!("Main coroutine spawning children");

                let child1_id = stack.spawn(move |child| {
                    simple_work(child, 1);
                });
                println!("Spawned child 1");

                let child2_id = stack.spawn(move |child| {
                    simple_work(child, 2);
                });
                println!("Spawned child 2");

                // Monitor the children
                println!("Main coroutine monitoring children");

                let mut done1 = false;
                let mut done2 = false;

                // Track all yielded values
                let mut values1 = Vec::new();
                let mut values2 = Vec::new();

                while !done1 || !done2 {
                    if !done1 {
                        match stack.get_yielded(child1_id) {
                            Some(value) => {
                                println!("Child 1 yielded {}", value);
                                values1.push(value);
                            }
                            None => {
                                println!("Child 1 complete");
                                done1 = true;
                            }
                        }
                    }

                    if !done2 {
                        match stack.get_yielded(child2_id) {
                            Some(value) => {
                                println!("Child 2 yielded {}", value);
                                values2.push(value);
                            }
                            None => {
                                println!("Child 2 complete");
                                done2 = true;
                            }
                        }
                    }
                }

                // Verify the sequences
                assert_eq!(
                    values1,
                    vec![1, 101, 201],
                    "Child 1 yielded incorrect sequence"
                );
                assert_eq!(
                    values2,
                    vec![2, 102, 202],
                    "Child 2 yielded incorrect sequence"
                );
            }),
        );

        println!("Adding stack to dispatcher");
        dispatcher.stacks.push(stack);
        println!("Starting dispatcher.run()");
        dispatcher.run();
        println!("Test complete");
    }

    #[test]
    fn test_chain_with_values() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        const MAX_DEPTH: u64 = 3; // Try three levels now

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                fn spawn_chain(stack: &mut Stack, depth: u64, max_depth: u64) {
                    // Always yield our depth first
                    stack.yield_value(depth as u128);

                    // Then spawn child if we're not at max depth
                    if depth < max_depth {
                        let child_id = stack.spawn(move |child| {
                            spawn_chain(child, depth + 1, max_depth);
                        });

                        // Forward all values from child
                        while let Some(value) = stack.get_yielded(child_id) {
                            stack.yield_value(value);
                        }
                    }
                }

                spawn_chain(stack, 0, MAX_DEPTH);
            }),
        );

        dispatcher.stacks.push(stack);
        dispatcher.run();
    }

    #[test]
    fn test_multiple_spawn() {
        let mut dispatcher = Dispatcher::new(Box::new(HeapAllocator::new()));
        let d = &mut dispatcher as *mut Dispatcher;

        let stack = Stack::new(
            0,
            d,
            Box::new(|stack| {
                println!("Parent: spawning first child");
                let child1_id = stack.spawn(|child| {
                    println!("Child 1: yielding 42");
                    child.yield_value(42);
                });

                println!("Parent: spawning second child");
                let child2_id = stack.spawn(|child| {
                    println!("Child 2: yielding 84");
                    child.yield_value(84);
                });

                // Get results from both children
                if let Some(value1) = stack.get_yielded(child1_id) {
                    println!("Parent: got {} from child 1", value1);
                    assert_eq!(value1, 42);
                } else {
                    println!("Parent: failed to get value from child 1");
                    assert!(false, "Failed to get value from child 1");
                }

                if let Some(value2) = stack.get_yielded(child2_id) {
                    println!("Parent: got {} from child 2", value2);
                    assert_eq!(value2, 84);
                } else {
                    println!("Parent: failed to get value from child 2");
                    assert!(false, "Failed to get value from child 2");
                }
            }),
        );

        dispatcher.stacks.push(stack);
        dispatcher.run();
    }
}

fn main() {
    // todo
}
