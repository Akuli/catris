use tokio::net::{TcpListener, TcpStream};
use std::net::IpAddr;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use std::time::Duration;
use std::sync::Arc;
use std::sync::Mutex;

struct ServerState {
    text_showing: bool,
}

type SafeServerState = Arc<Mutex<ServerState>>;

async fn flipper(safe_state: SafeServerState) {
    loop {
        {
            let mut state = safe_state.lock().unwrap();
            state.text_showing = !state.text_showing;
            println!("Flipped: {}", state.text_showing)
        }
        sleep(Duration::from_secs(1)).await;
    }
}

async fn process(socket: &mut TcpStream, ip: IpAddr, safe_state: SafeServerState) {
    println!("Processing!!! {}", ip);
    loop {
        let text_showing: bool;
        {
            let state = safe_state.lock().unwrap();
            text_showing = state.text_showing;
        }
        if text_showing {
            socket.write(b"hello\r\n").await.unwrap();  // FIXME: don't panic
        } else {
            socket.write(b"lolwat\r\n").await.unwrap();  // FIXME: don't panic
        }
        sleep(Duration::from_millis(200)).await;
    }
}

#[tokio::main]
async fn main() {
    // Bind the listener to the address
    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();
    println!("Listening!");

    let safe_state = Arc::new(Mutex::new(ServerState{text_showing: false}));
    tokio::spawn(flipper(safe_state.clone()));

    loop {
        let (mut socket, sockaddr) = listener.accept().await.unwrap();
        let safe_state = safe_state.clone();
        tokio::spawn(async move {
            process(&mut socket, sockaddr.ip(), safe_state).await;
        });
    }
}

