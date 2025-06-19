use std::sync::{Arc};
use std::thread;
use std::net::{TcpListener, TcpStream};
use std::io::{Read, Write};

use crate::shared_register::AtomicRegister;

fn handle_client(mut stream: TcpStream, atomic_register: Arc<AtomicRegister>) {
    let mut buffer = [0; 1024];
    stream.read(&mut buffer).unwrap();
    let request = String::from_utf8_lossy(&buffer[..]);

    let mut response = String::new();
    response.push_str("HTTP/1.1 200 OK\r\n");
    response.push_str("Content-Type: text/plain\r\n");
    response.push_str("\r\n");

    let mut iter = request.trim().split_whitespace();
    let command = iter.nth(1).unwrap_or_default();

    let response_body = match command {
        "/read" => atomic_register.read(),  
        "/write" => {
            let value = request.split("\r\n\r\n").nth(1).unwrap_or("new value");
            let response_string = atomic_register.write(String::from(value.trim()));
            response_string
        },
        "/write_with_quorum" => {
            let value = request.split("\r\n\r\n").nth(1).unwrap_or("new value");
            let response_string = atomic_register.write_with_quorum(String::from(value));
            response_string
        },
        _ => String::from("Invalid request"),
    };
    response.push_str(&response_body);

    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}

// We use Arc here because we need different threads of operations to see the same AtomicRegister
pub fn start_server(port: u32, atomic_register: Arc<AtomicRegister>) {
    println!("Starting server on port {}", port);
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).unwrap();
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let atomic_register = atomic_register.clone();
                thread::spawn(move || {
                    handle_client(stream, atomic_register);
                });
            }
            Err(e) => {
                println!("Error: {}", e);
            }
        }
    }
}
