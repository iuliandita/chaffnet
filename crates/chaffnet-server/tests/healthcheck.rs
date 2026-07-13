use chaffnet_server::healthcheck::{check_endpoint, HealthcheckError};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

fn response_server(status: &str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let response = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n");
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 256];
        let size = stream.read(&mut request).unwrap();
        assert!(String::from_utf8_lossy(&request[..size]).starts_with("GET /healthz HTTP/1.1"));
        stream.write_all(response.as_bytes()).unwrap();
    });
    (address, handle)
}

#[test]
fn healthcheck_accepts_successful_health_endpoint() {
    let (address, server) = response_server("200 OK");
    check_endpoint(&address, Duration::from_secs(1)).unwrap();
    server.join().unwrap();
}

#[test]
fn healthcheck_rejects_non_success_status() {
    let (address, server) = response_server("503 Service Unavailable");
    let error = check_endpoint(&address, Duration::from_secs(1)).unwrap_err();
    assert!(matches!(error, HealthcheckError::UnhealthyStatus(_)));
    server.join().unwrap();
}

#[test]
fn wildcard_bind_address_is_probed_over_loopback() {
    let (address, server) = response_server("200 OK");
    let port = address.rsplit_once(':').unwrap().1;
    check_endpoint(&format!("0.0.0.0:{port}"), Duration::from_secs(1)).unwrap();
    server.join().unwrap();
}
