use tokio::net::{TcpListener, TcpStream};
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Bind the listener to the address
    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();
    println!("Listening!");

    loop {
        let (mut socket, sockaddr) = listener.accept().await.unwrap();
        process(&mut socket, sockaddr).await;
    }
}

async fn process(socket: &mut TcpStream, sockaddr: SocketAddr) {
    println!("Processing!!! {}", sockaddr.ip());
    socket.write(b"hello\r\n").await.unwrap();  // FIXME: don't panic
    socket.write(b"lolwat\r\n").await.unwrap();  // FIXME: don't panic
    sleep(Duration::from_secs(10)).await;
}
