use crate::ansi::parse_key_press;
use crate::ansi::KeyPress;
use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::collections::VecDeque;
use std::io;
use std::io::ErrorKind;
use std::time::Duration;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

// Errors can be io::Error or tungstenite::Error.
// I can't box them because boxes aren't Send i.e. can't be held across await.
fn convert_error(e: tungstenite::Error) -> io::Error {
    io::Error::new(ErrorKind::Other, format!("websocket error: {:?}", e))
}

fn connection_closed_error() -> io::Error {
    io::Error::new(ErrorKind::ConnectionAborted, "connection closed")
}

fn check_key_press_frequency(key_press_times: &mut VecDeque<Instant>) -> Result<(), io::Error> {
    key_press_times.push_back(Instant::now());
    while key_press_times.len() != 0 && key_press_times[0].elapsed().as_secs_f32() > 1.0 {
        key_press_times.pop_front();
    }
    if key_press_times.len() > 100 {
        return Err(io::Error::new(
            ErrorKind::ConnectionAborted,
            "received more than 100 key presses / sec",
        ));
    }
    Ok(())
}

pub enum Receiver {
    WebSocket {
        ws_reader: SplitStream<WebSocketStream<TcpStream>>,
        pings: mpsc::Sender<Vec<u8>>,
        key_press_times: VecDeque<Instant>,
    },
    RawTcp {
        read_half: OwnedReadHalf,
        buffer: [u8; 100], // keep small, receiving a single key press is O(recv buffer size)
        buffer_size: usize,
        key_press_times: VecDeque<Instant>,
    },
}
impl Receiver {
    pub async fn receive_key_press(&mut self) -> Result<KeyPress, io::Error> {
        match self {
            Self::WebSocket {
                ws_reader,
                pings,
                key_press_times,
            } => {
                loop {
                    let item = ws_reader.next().await;
                    if item.is_none() {
                        return Err(connection_closed_error());
                    }

                    // Receiving anything from the websocket counts as a key press
                    check_key_press_frequency(key_press_times)?;

                    match item.unwrap().map_err(convert_error)? {
                        Message::Binary(bytes) => {
                            match parse_key_press(&bytes) {
                                // Websocket never splits a key press to multiple messages.
                                // Also can't have multiple key presses inside the same message.
                                Some((key, n)) if n == bytes.len() => {
                                    return Ok(key);
                                }
                                Some(_) | None => {
                                    return Err(io::Error::new(
                                        ErrorKind::Other,
                                        "received bad key press from websocket",
                                    ))
                                }
                            }
                        }
                        Message::Close(_) => {
                            return Err(connection_closed_error());
                        }
                        Message::Text(_) => {
                            // web ui uses binary messages for everything
                            return Err(io::Error::new(
                                ErrorKind::Other,
                                "received unexpected websocket text frame",
                            ));
                        }
                        Message::Ping(bytes) => {
                            pings.send(bytes).await.map_err(|_| {
                                io::Error::new(
                                // hopefully this error never actually happens
                                ErrorKind::Other,
                                "can't respond to websocket ping because sending task has stopped"
                            )
                            })?;
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

            Self::RawTcp {
                read_half,
                buffer,
                buffer_size,
                key_press_times,
            } => {
                loop {
                    match parse_key_press(&buffer[..*buffer_size]) {
                        Some((key, bytes_used)) => {
                            check_key_press_frequency(key_press_times)?;
                            buffer[..*buffer_size].rotate_left(bytes_used);
                            *buffer_size -= bytes_used;
                            return Ok(key);
                        }
                        None => {
                            // Receive more data
                            let dest = &mut buffer[*buffer_size..];
                            let n = timeout(Duration::from_secs(10 * 60), read_half.read(dest))
                                .await??;
                            if n == 0 {
                                return Err(connection_closed_error());
                            }
                            *buffer_size += n;
                        }
                    }
                }
            }
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
        let config = WebSocketConfig {
            // Prevent various denial-of-service attacks that fill up server's memory.
            // Most defaults are reasonable, but unnecessarily huge for this program.
            max_send_queue: Some(10), // TODO: can be 1? https://github.com/snapview/tungstenite-rs/issues/285
            max_message_size: Some(1024),
            max_frame_size: Some(1024),
            ..Default::default()
        };
        let ws = tokio_tungstenite::accept_async_with_config(socket, Some(config))
            .await
            .map_err(convert_error)?;
        let (ws_writer, ws_reader) = ws.split();
        sender = Sender::WebSocket { ws_writer };
        receiver = Receiver::WebSocket {
            ws_reader,
            pings: ws_ping_sender,
            key_press_times: VecDeque::new(),
        };
    } else {
        let (read_half, write_half) = socket.into_split();
        sender = Sender::RawTcp { write_half };
        receiver = Receiver::RawTcp {
            read_half,
            buffer: [0u8; 100],
            buffer_size: 0,
            key_press_times: VecDeque::new(),
        };
    }

    Ok((sender, receiver, ws_ping_receiver))
}
