use std::sync::{Arc, atomic::{AtomicPtr}};
use std::thread;
use std::env;

mod shared_register;
mod server;

use crate::shared_register::AtomicRegister;
use crate::server::start_server;

// Trying to demo usage of multiple servers running a shared AtomicRegister
fn main() {
    let nodes: Vec<String> = vec!["localhost:8000", "localhost:8001", "localhost:8002", "localhost:8003"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let register = Arc::new(AtomicPtr::new(Box::into_raw(Box::new("".to_string()))));

    let args: Vec<String> = env::args().collect();
    let port = match args.len() {
        2 => args[1].parse().unwrap(),
        _ => {
            println!("Please provide a port number as an argument");
            return;
        }
    };

    let atomic_register = Arc::new(AtomicRegister::new(port, nodes.clone(), register.clone()));
    let handle = thread::spawn(move || {
        let _server = start_server(port, atomic_register);
        println!("Server listening on port {}", port);
    });
    
    handle.join().unwrap();
}
