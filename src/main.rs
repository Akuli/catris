/*use tokio::net::{TcpListener, TcpStream};
use std::net::IpAddr;
use tokio::io::AsyncWriteExt;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::watch;
*/
use std::time::Duration;
use tokio::time::sleep;

mod game_logic;

/*
struct ServerState {
    flag: bool,
    update_sender: watch::Sender<()>,
}

type SafeServerState = Arc<Mutex<ServerState>>;

async fn flipper(safe_state: SafeServerState) {
    loop {
        {
            let mut state = safe_state.lock().unwrap();
            state.flag = !state.flag;
            state.update_sender.send(()).unwrap();
            state.update_sender.send(()).unwrap();
        }
        sleep(Duration::from_secs(1)).await;
    }
}

async fn process(socket: &mut TcpStream, ip: IpAddr, safe_state: SafeServerState) {
    println!("Connection from {}", ip);
    let mut receiver = safe_state.lock().unwrap().update_sender.subscribe();
    loop {
        let flag: bool;
        {
            let state = safe_state.lock().unwrap();
            flag = state.flag;
        }
        if flag {
            socket.write(b"true\r\n").await.unwrap();
        } else {
            socket.write(b"false\r\n").await.unwrap();
        }
        receiver.changed().await.unwrap();
    }
}
*/

#[tokio::main]
async fn main() {
    /*
    let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();

    let (sender, receiver) = watch::channel(());
    let safe_state = Arc::new(Mutex::new(ServerState{update_sender: sender, flag: false}));
    tokio::spawn(flipper(safe_state.clone()));

    loop {
        let (mut socket, sockaddr) = listener.accept().await.unwrap();
        let safe_state = safe_state.clone();
        tokio::spawn(async move {
            process(&mut socket, sockaddr.ip(), safe_state).await;
        });
    }*/

    let block = game_logic::MovingBlock{
        center_x: 5,
        center_y: -1,
        relative_coords: vec![(0,0),(0,-1),(-1,0),(-1,-1)],
    };
    let player = game_logic::Player{
        name: "Foo".to_string(),
        block: block,
    };
    println!("name = {}", player.name);
    let mut game = game_logic::Game{
        players: vec![player],
    };

    for _ in 1..10 {
        println!("\x1b[2J");
        for line in game.get_lines_to_render() {
            println!("{}", line);
        }
        sleep(Duration::from_millis(400)).await;
        game.move_blocks_down();
    }
}
