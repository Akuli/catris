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
    while !key_press_times.is_empty() && key_press_times[0].elapsed().as_secs_f32() > 1.0 {
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

fn get_timeout(last_recv: Instant) -> Duration {
    let deadline = last_recv + Duration::from_secs(10 * 60);
    deadline.saturating_duration_since(Instant::now())
}

pub enum Receiver {
    WebSocket {
        ws_reader: SplitStream<WebSocketStream<TcpStream>>,
        key_press_times: VecDeque<Instant>,
        last_recv: Instant,
    },
    RawTcp {
        read_half: OwnedReadHalf,
        buffer: [u8; 100], // keep small, receiving a single key press is O(recv buffer size)
        buffer_size: usize,
        key_press_times: VecDeque<Instant>,
        last_recv: Instant,
    },
    Test(String),
}
impl Receiver {
    pub async fn receive_key_press(&mut self) -> Result<KeyPress, io::Error> {
        match self {
            Self::WebSocket {
                ws_reader,
                key_press_times,
                last_recv,
            } => {
                loop {
                    let item = timeout(get_timeout(*last_recv), ws_reader.next()).await?;
                    if item.is_none() {
                        return Err(connection_closed_error());
                    }
                    let item = item.unwrap();
                    check_key_press_frequency(key_press_times)?;

                    match item.map_err(convert_error)? {
                        Message::Binary(bytes) => {
                            *last_recv = Instant::now();
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
                        /*
                        Pings can't make the connection stay open for more than 10min.
                        That would cause confusion when people use different browsers and
                        not all browsers send pings.

                        Pings are counted as key presses, so that you will be disconnected
                        if you spam the server with lots of pings.

                        We don't have to send pongs, because tungstenite does it
                        automatically.
                        */
                        Message::Ping(_) => {}
                        other => {
                            return Err(io::Error::new(
                                ErrorKind::Other,
                                format!("unexpected websocket frame: {:?}", other),
                            ));
                        }
                    }
                }
            }

            Self::RawTcp {
                read_half,
                buffer,
                buffer_size,
                key_press_times,
                last_recv,
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
                            let n =
                                timeout(get_timeout(*last_recv), read_half.read(dest)).await??;
                            if n == 0 {
                                return Err(connection_closed_error());
                            }
                            *buffer_size += n;
                            *last_recv = Instant::now();
                        }
                    }
                }
            }

            Self::Test(string) => match parse_key_press(string.as_bytes()) {
                Some((key, bytes_used)) => {
                    *string = string[bytes_used..].to_string();
                    Ok(key)
                }
                None => Err(connection_closed_error()),
            },
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
    pub async fn send(&mut self, data: &[u8]) -> Result<(), io::Error> {
        match self {
            Self::WebSocket { ws_writer } => ws_writer
                .send(Message::binary(data.to_vec()))
                .await
                .map_err(convert_error),
            Self::RawTcp { write_half } => write_half.write_all(data).await,
        }
    }
}

pub async fn initialize_connection(
    socket: TcpStream,
    is_websocket: bool,
) -> Result<(Sender, Receiver), io::Error> {
    /*
    Tell the kernel to prefer sending in small pieces, as soon as possible.

    The kernel buffers the data, and by default, sends in large packets.
    This makes sending big amounts of data more efficient, but this program
    sends several small screen updates. They should be sent independently,
    as a stream of data, not combined into large batches.

    Without this option, connecting with 50ms ping is enough to make things
    not look as smooth as they should be, especially quickly falling blocks.
    */
    socket.set_nodelay(true)?;

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
            key_press_times: VecDeque::new(),
            last_recv: Instant::now(),
        };
    } else {
        let (read_half, write_half) = socket.into_split();
        sender = Sender::RawTcp { write_half };
        receiver = Receiver::RawTcp {
            read_half,
            buffer: [0u8; 100],
            buffer_size: 0,
            key_press_times: VecDeque::new(),
            last_recv: Instant::now(),
        };
    }

    Ok((sender, receiver))
}
