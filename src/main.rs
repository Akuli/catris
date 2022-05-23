use tokio::net::{TcpListener, TcpStream};
use std::net::IpAddr;
use tokio::io::AsyncWriteExt;
use tokio::time::sleep;
use std::time::Duration;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::watch;

struct ServerState {
    text_showing: bool,
    update_sender: watch::Sender<()>,
}

type SafeServerState = Arc<Mutex<ServerState>>;

async fn flipper(safe_state: SafeServerState) {
    loop {
        {
            let mut state = safe_state.lock().unwrap();
            state.text_showing = !state.text_showing;
            println!("Flipped: {}", state.text_showing);
            _ = state.update_sender.send(());  // TODO: why this failing?
        }
        sleep(Duration::from_secs(1)).await;
    }
}

async fn process(socket: &mut TcpStream, ip: IpAddr, safe_state: SafeServerState) {
    println!("Processing!!! {}", ip);
    let mut receiver = safe_state.lock().unwrap().update_sender.subscribe();
    loop {
        let text_showing: bool;
        {
            let state = safe_state.lock().unwrap();
            text_showing = state.text_showing;
        }
        if text_showing {
            socket.write(b"true\r\n").await.unwrap();  // FIXME: don't panic
        } else {
            socket.write(b"false\r\n").await.unwrap();  // FIXME: don't panic
        }

        receiver.changed().await.unwrap();
    }
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();
    println!("Listening!");

    let (sender, _) = watch::channel(());
    let safe_state = Arc::new(Mutex::new(ServerState{update_sender: sender, text_showing: false}));
    tokio::spawn(flipper(safe_state.clone()));

    loop {
        let (mut socket, sockaddr) = listener.accept().await.unwrap();
        let safe_state = safe_state.clone();
        tokio::spawn(async move {
            process(&mut socket, sockaddr.ip(), safe_state).await;
        });
    }
}
