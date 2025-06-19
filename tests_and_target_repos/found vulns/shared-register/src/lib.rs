pub mod shared_register;
pub mod server;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::{AtomicPtr}};
    use std::thread;
    use std::time::Duration;
    use reqwest::blocking::Client;


    use crate::shared_register::AtomicRegister;
    use crate::server::start_server;
    
    #[test]
    fn test_handle_client() {
        let register = Arc::new(AtomicPtr::new(Box::into_raw(Box::new("".to_string()))));
        let atomic_register = Arc::new(AtomicRegister::new(8080, vec!["127.0.0.1:8080".to_string()], register.clone()));
        thread::spawn(move || {
            start_server(8080, atomic_register.clone());
        });
        thread::sleep(Duration::from_millis(100));
        let client = Client::new();
        let response = client.post("http://127.0.0.1:8080/write").body("test message").send().unwrap_or_else(|err| {
            eprintln!("Error sending request: {}", err);
            panic!("Failed to send request");
        }).text().unwrap();
        println!("Response from server: {}", response);
        assert_eq!(response, "ACK");
    }

    #[test]
    fn test_handle_client_read() {
        let register = Arc::new(AtomicPtr::new(Box::into_raw(Box::new("".to_string()))));
        let atomic_register = Arc::new(AtomicRegister::new(8080, vec!["127.0.0.1:8080".to_string()], register.clone()));
        thread::spawn(move || {
            start_server(8080, atomic_register.clone());
        });
        thread::sleep(Duration::from_millis(100));
        let client = Client::new();
        let write_response = client.post("http://127.0.0.1:8080/write").body("test message").send().unwrap_or_else(|err| {
            eprintln!("Error sending request: {}", err);
            panic!("Failed to send request");
        }).text().unwrap();
        println!("Response from server: {}", write_response);
        assert_eq!(write_response, "ACK");
        let read_response = client.get("http://127.0.0.1:8080/read").send().unwrap_or_else(|err| {
            eprintln!("Error sending request: {}", err);
            panic!("Failed to send request");
        }).text().unwrap();
        println!("Response from server: {}", read_response);
        assert_eq!(read_response, "test message");
    }
    
    #[test]
    fn test_quorum_protocol() {
        let register = Arc::new(AtomicPtr::new(Box::into_raw(Box::new("".to_string()))));
        let atomic_register1 = Arc::new(AtomicRegister::new(8081, vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()], register.clone()));
        let atomic_register2 = Arc::new(AtomicRegister::new(8082, vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()], register.clone()));
        let atomic_register3 = Arc::new(AtomicRegister::new(8083, vec!["127.0.0.1:8081".to_string(), "127.0.0.1:8082".to_string(), "127.0.0.1:8083".to_string()], register.clone()));
        thread::spawn(move || {
            start_server(8081, atomic_register1.clone());
        });
        thread::spawn(move || {
            start_server(8082, atomic_register2.clone());
        });
        thread::spawn(move || {
            start_server(8083, atomic_register3.clone());
        });
        thread::sleep(Duration::from_millis(100));
        let client = Client::new();
        let response = client.post("http://127.0.0.1:8081/write_with_quorum").body("test message").send().unwrap().text().unwrap();
        assert_eq!(response, "ACK");
        thread::sleep(Duration::from_millis(100));
        let client = Client::new();
        let response1 = client.get("http://127.0.0.1:8081/read").send().unwrap_or_else(|err| {
            eprintln!("Error sending request: {}", err);
            panic!("Failed to send request");
        }).text().unwrap();
        assert_eq!(response1, "test message");
        let response2 = client.get("http://127.0.0.1:8082/read").send().unwrap_or_else(|err| {
            eprintln!("Error sending request: {}", err);
            panic!("Failed to send request");
        }).text().unwrap();
        assert_eq!(response2, "test message");
        let response3 = client.get("http://127.0.0.1:8083/read").send().unwrap_or_else(|err| {
            eprintln!("Error sending request: {}", err);
            panic!("Failed to send request");
        }).text().unwrap();
        assert_eq!(response3, "test message");

        let response4 = client.get("http://127.0.0.1:8081/read").send().unwrap_or_else(|err| {
            eprintln!("Error sending request: {}", err);
            panic!("Failed to send request");
        }).text().unwrap();
        assert_eq!(response4, "test message");
    }

}