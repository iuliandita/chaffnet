use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum HealthcheckError {
    #[error("invalid CHAFFNET_BIND address {0:?}")]
    InvalidAddress(String),
    #[error("health endpoint I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("health endpoint returned an unhealthy status: {0}")]
    UnhealthyStatus(String),
}

pub fn check_endpoint(bind: &str, timeout: Duration) -> Result<(), HealthcheckError> {
    let mut address: SocketAddr = bind
        .parse()
        .map_err(|_| HealthcheckError::InvalidAddress(bind.into()))?;
    if address.ip().is_unspecified() {
        address.set_ip(match address.ip() {
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        });
    }

    let mut stream = TcpStream::connect_timeout(&address, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;

    let mut status_line = String::new();
    BufReader::new(stream.take(1024)).read_line(&mut status_line)?;
    if status_line.split_whitespace().nth(1) != Some("200") {
        return Err(HealthcheckError::UnhealthyStatus(
            status_line.trim().to_string(),
        ));
    }
    Ok(())
}
