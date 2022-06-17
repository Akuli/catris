use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::io;
use std::io::ErrorKind;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

// Errors can be io::Error or tungstenite::Error.
// I can't box them because boxes aren't Send i.e. can't be held across await.
fn convert_error(e: tungstenite::Error) -> io::Error {
    io::Error::new(ErrorKind::Other, format!("websocket error: {:?}", e))
}

pub enum Receiver {
    WebSocket {
        ws_reader: SplitStream<WebSocketStream<TcpStream>>,
        pings: mpsc::Sender<Vec<u8>>,
    },
    RawTcp {
        read_half: OwnedReadHalf,
    },
}
impl Receiver {
    pub async fn receive_into(&mut self, target: &mut [u8]) -> Result<usize, io::Error> {
        match self {
            Self::WebSocket { ws_reader, pings } => {
                loop {
                    let item = ws_reader.next().await;
                    if item.is_none() {
                        return Ok(0); // connection closed
                    }
                    match item.unwrap().map_err(convert_error)? {
                        Message::Binary(bytes) => {
                            // TODO: what if client send 1GB message of bytes at once?
                            // would be already fucked up, because Vec<u8> of 1GB was allocated
                            if bytes.len() > target.len() {
                                return Err(io::Error::new(
                                    ErrorKind::Other,
                                    format!(
                                        "too long websocket message: {} > {}",
                                        bytes.len(),
                                        target.len()
                                    ),
                                ));
                            }
                            for i in 0..bytes.len() {
                                target[i] = bytes[i];
                            }
                            return Ok(bytes.len());
                        }
                        Message::Close(_) => {
                            return Ok(0);
                        }
                        Message::Text(_) => {
                            // web ui uses binary messages for everything
                            return Err(io::Error::new(
                                ErrorKind::Other,
                                "received unexpected websocket text frame",
                            ));
                        }
                        Message::Ping(bytes) => {
                            // TODO: rate limit
                            pings
                                .send(bytes)
                                .await
                                .map_err(|_| io::Error::new(
                                    ErrorKind::Other,
                                    "can't respond to websocket ping because sending task has stopped"
                                ))?;
                        }
                        Message::Pong(_) => {
                            // we never send ping, so client should never send pong
                            return Err(io::Error::new(
                                ErrorKind::Other,
                                "unexpected websocket pong",
                            ));
                        }
                        Message::Frame(_) => {
                            panic!("this is impossible according to docs");
                        }
                    }
                }
            }
            Self::RawTcp { read_half } => read_half.read(target).await,
        }
    }
}

pub enum Sender {
    WebSocket {
        ws_writer: SplitSink<WebSocketStream<tokio::net::TcpStream>, Message>,
    },
    RawTcp {
        write_half: OwnedWriteHalf,
    },
}
impl Sender {
    pub async fn send_message(&mut self, data: &[u8]) -> Result<(), io::Error> {
        match self {
            Self::WebSocket { ws_writer } => ws_writer
                .send(Message::binary(data.to_vec()))
                .await
                .map_err(convert_error),
            Self::RawTcp { write_half } => write_half.write_all(data).await,
        }
    }

    pub async fn send_websocket_ping(&mut self, ping_data: Vec<u8>) -> Result<(), io::Error> {
        match self {
            Self::WebSocket { ws_writer } => ws_writer
                .send(Message::Ping(ping_data))
                .await
                .map_err(convert_error),
            Self::RawTcp { .. } => panic!(),
        }
    }
}

pub async fn initialize_connection(
    socket: TcpStream,
    is_websocket: bool,
) -> Result<(Sender, Receiver, mpsc::Receiver<Vec<u8>>), io::Error> {
    // Support websocket pings just in case someone's browser uses them.
    // We don't really need them for anything, because the server uses tcp keepalive.
    let (ws_ping_sender, ws_ping_receiver) = mpsc::channel(3);
    let sender;
    let receiver;

    if is_websocket {
        let ws = tokio_tungstenite::accept_async(socket)
            .await
            .map_err(convert_error)?;
        let (ws_writer, ws_reader) = ws.split();
        sender = Sender::WebSocket { ws_writer };
        receiver = Receiver::WebSocket {
            ws_reader,
            pings: ws_ping_sender,
        };
    } else {
        let (read_half, write_half) = socket.into_split();
        sender = Sender::RawTcp { write_half };
        receiver = Receiver::RawTcp { read_half };
    }

    Ok((sender, receiver, ws_ping_receiver))
}
