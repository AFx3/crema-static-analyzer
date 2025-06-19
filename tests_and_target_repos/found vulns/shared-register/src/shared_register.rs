use std::collections::HashMap;
use std::sync::{Arc};
use std::thread;
use std::time::{Duration, Instant};
use std::sync::atomic::{AtomicPtr, Ordering};

#[derive(Clone)]
pub struct AtomicRegister {
    id: u32,
    nodes: Vec<String>,
    register: Arc<AtomicPtr<String>>,
}

impl AtomicRegister {
    pub fn new(id: u32, nodes: Vec<String>, register: Arc<AtomicPtr<String>>) -> Self {
        Self {
            id,
            nodes,
            register,
        }
    }

    pub fn read(&self) -> String {
        let mut register = self.register.load(Ordering::SeqCst);

        // this should make a read operation not return in so far as there are write operations changing the value
        loop {
            let latest_register = self.register.load(Ordering::SeqCst);
            if register == latest_register {
                break;
            }
            register = latest_register;
        }

        // TODO: Should serialize the messages instead of doing shit like this
        unsafe { (*register).clone().trim_matches('\0').to_string() } 
    }

    pub fn write(&self, value: String) -> String {
        println!("Value received in write: {}", value);
        let mut register = self.register.load(Ordering::SeqCst);
        let new_register = AtomicPtr::new(Box::into_raw(Box::new(value)));
        loop {
            match self.register.compare_exchange(register, new_register.load(Ordering::SeqCst), Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(current_register) => {
                    register = current_register;
                }
            }
        }

        "ACK".to_string()
    }

    pub fn write_with_quorum(&self, value: String) -> String {
        let mut ack_count = 0;
        let mut responses = HashMap::new();
        let quorum_size = self.nodes.len() / 2;
    
        for node in &self.nodes {
            let node_port = node.split(":").last().unwrap();
            let self_id = self.id.clone().to_string();
            let self_port = self_id.split(":").last().unwrap();
            if node_port != self_port {
                let url = format!("http://{}/write", node);
                let client = reqwest::blocking::Client::new();
                let response = client
                    .post(&url)
                    .body(value.clone())
                    .send()
                    .unwrap()
                    .text()
                    .unwrap();
                responses.insert(node, response);
            } 
        }

        let timeout_duration = Duration::from_secs(5); // wait for 5 seconds
        let start_time = Instant::now();
    
        loop {
            for (node, response) in &responses {
                if response == "ACK" {
                    ack_count += 1;
                    println!("Received ACK from node: {}", node);
                } else {
                    println!("Received response from node {}: {}", node, response);
                }
            }
    
            if ack_count >= quorum_size {
                self.write(value);
                return "ACK".to_string();
            }
    
            if start_time.elapsed() >= timeout_duration {
                break;
            }
    
            // Wait for a short time before checking again
            thread::sleep(Duration::from_millis(100));
        }
    
        "ERROR: Quorum not reached".to_string()
    }
}

