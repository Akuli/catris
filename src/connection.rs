use crate::escapes::parse_key_press;
use crate::escapes::KeyPress;
use crate::ip_tracker::ForgetClientOnDrop;
use crate::ip_tracker::IpTracker;
use crate::ClientLogger;
use futures_util::stream::SplitSink;
use futures_util::stream::SplitStream;
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::collections::VecDeque;
use std::env;
use std::io;
use std::io::ErrorKind;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::tungstenite::handshake::server::Callback;
use tokio_tungstenite::tungstenite::handshake::server::ErrorResponse;
use tokio_tungstenite::tungstenite::handshake::server::Request;
use tokio_tungstenite::tungstenite::handshake::server::Response;
use tokio_tungstenite::tungstenite::http;
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

pub fn get_websocket_proxy_ip() -> Option<IpAddr> {
    let string = env::var("CATRIS_WEBSOCKET_PROXY_IP").unwrap_or("".to_string());
    if string.is_empty() {
        None
    } else {
        Some(string.parse().unwrap())
    }
}

// Errors can be io::Error or tungstenite::Error.
// I can't box them because boxes aren't Send i.e. can't be held across await.
fn convert_error(e: tungstenite::Error) -> io::Error {
    io::Error::new(ErrorKind::Other, format!("websocket error: {:?}", e))
}

fn connection_closed_error() -> io::Error {
    io::Error::new(ErrorKind::ConnectionAborted, "connection closed")
}

pub struct ReceiveState {
    buffer: VecDeque<u8>,
    key_press_times: VecDeque<Instant>,
    last_recv: Instant,
}
impl ReceiveState {
    fn add_received_bytes(&mut self, bytes: &[u8]) {
        // Receiving empty bytes has a special meaning in raw TCP and should never happen in websocket
        assert!(!bytes.is_empty());

        for byte in bytes {
            self.buffer.push_back(*byte);
        }
        self.last_recv = Instant::now();
    }

    fn get_timeout(&self) -> Duration {
        let deadline = self.last_recv + Duration::from_secs(10 * 60);
        deadline.saturating_duration_since(Instant::now())
    }

    fn check_key_press_frequency(&mut self) -> Result<(), io::Error> {
        self.key_press_times.push_back(Instant::now());
        while !self.key_press_times.is_empty()
            && self.key_press_times[0].elapsed().as_secs_f32() > 1.0
        {
            self.key_press_times.pop_front();
        }

        if self.key_press_times.len() > 100 {
            return Err(io::Error::new(
                ErrorKind::ConnectionAborted,
                "received more than 100 key presses / sec",
            ));
        }
        Ok(())
    }
}

pub enum Receiver {
    WebSocket {
        ws_reader: SplitStream<WebSocketStream<TcpStream>>,
        recv_state: ReceiveState,
    },
    RawTcp {
        read_half: OwnedReadHalf,
        recv_state: ReceiveState,
    },
    #[allow(dead_code)]
    Test(String),
}
impl Receiver {
    async fn receive_more_data(&mut self) -> Result<(), io::Error> {
        match self {
            Self::WebSocket {
                recv_state,
                ws_reader,
            } => {
                let item = timeout(recv_state.get_timeout(), ws_reader.next())
                    .await? // error if timed out
                    .ok_or_else(connection_closed_error)? // error if clean disconnect
                    .map_err(convert_error)?; // error if receiving failed

                match item {
                    Message::Binary(bytes) => {
                        if bytes.is_empty() {
                            Err(io::Error::new(
                                ErrorKind::Other,
                                "received empty bytes from websocket message",
                            ))
                        } else {
                            recv_state.add_received_bytes(&bytes);
                            Ok(())
                        }
                    }
                    Message::Close(_) => Err(connection_closed_error()),
                    /*
                    Pings can't make the connection stay open for more than 10min.
                    That would cause confusion when people use different browsers and
                    not all browsers send pings.

                    Pings are counted as key presses, so that you will be disconnected
                    if you spam the server with lots of pings.

                    We don't have to send pongs, because tungstenite does it
                    automatically.
                    */
                    Message::Ping(_) => {
                        recv_state.check_key_press_frequency()?;
                        Ok(())
                    }
                    other => Err(io::Error::new(
                        ErrorKind::Other,
                        format!("unexpected websocket frame: {:?}", other),
                    )),
                }
            }
            Self::RawTcp {
                recv_state,
                read_half,
            } => {
                let mut buf = [0u8; 100];

                let n = timeout(recv_state.get_timeout(), read_half.read(&mut buf)).await??;
                if n == 0 {
                    // a clean disconnect
                    return Err(connection_closed_error());
                }
                recv_state.add_received_bytes(&buf[0..n]);
                Ok(())
            }
            _ => panic!(),
        }
    }

    pub async fn receive_key_press(&mut self) -> Result<KeyPress, io::Error> {
        match self {
            Self::Test(string) => {
                if string == "BLOCK" {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
                return match parse_key_press(string.as_bytes()) {
                    Some((key, bytes_used)) => {
                        *string = string[bytes_used..].to_string();
                        Ok(key)
                    }
                    None => Err(connection_closed_error()),
                };
            }
            _ => {}
        }

        loop {
            let received_so_far: &[u8] = match self {
                Self::Test(_) => panic!(),
                Self::WebSocket { recv_state, .. } | Self::RawTcp { recv_state, .. } => {
                    recv_state.buffer.make_contiguous();
                    recv_state.buffer.as_slices().0
                }
            };

            match parse_key_press(received_so_far) {
                Some((key, bytes_used)) => {
                    let recv_state = match self {
                        Self::Test(_) => panic!(),
                        Self::WebSocket { recv_state, .. } | Self::RawTcp { recv_state, .. } => {
                            recv_state
                        }
                    };
                    recv_state.check_key_press_frequency()?;
                    recv_state.buffer.drain(0..bytes_used);
                    return Ok(key);
                }
                None => self.receive_more_data().await?,
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

/*
tokio-tungstenite offers a callback trait that gets called when connecting.
Two WTF's here: 1) why is async library using callbacks? 2) why is it a trait and not FnMut?
It is the only way to access the headers from nginx...

To test that this gets the ip, create a file req.txt with this in it and TWO BLANK LINES after.
This file represents what nginx would sends when proxying.

GET /ws HTTP/1.1
Host: localhost
X-Real-IP: 12.34.56.78
Connection: upgrade
Upgrade: WebSocket
Sec-WebSocket-Version: 13
Sec-WebSocket-Key: hello

In one terminal:

    $ CATRIS_WEBSOCKET_PROXY_IP=127.0.0.1 cargo r

In another terminal:

    $ nc localhost 54321 < req.txt
    $ nc localhost 54321 < req.txt
    $ nc localhost 54321 < req.txt
    $ nc localhost 54321 < req.txt
    $ nc localhost 54321 < req.txt
    $ nc localhost 54321 < req.txt

You should see the dummy IP 12.34.56.78 printed.
*/
struct CheckRealIpCallback {
    logger: ClientLogger,
    ip_tracker: Arc<Mutex<IpTracker>>,
    decrementers: Vec<ForgetClientOnDrop>,
}
impl Callback for &mut CheckRealIpCallback {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        let ip: IpAddr = request
            .headers()
            .get("X-Real-IP")
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                http::Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Some(
                        "missing X-Real-IP header (should be set by proxy)".to_string(),
                    ))
                    .unwrap()
            })?
            .parse()
            .map_err(|_| {
                http::Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Some("cannot parse X-Real-IP header".to_string()))
                    .unwrap()
            })?;

        self.decrementers.push(
            IpTracker::track(self.ip_tracker.clone(), ip, self.logger).map_err(|_| {
                http::Response::builder()
                    .status(StatusCode::TOO_MANY_REQUESTS)
                    .body(None)
                    .unwrap()
            })?,
        );
        Ok(response)
    }
}

pub async fn initialize_connection(
    ip_tracker: Arc<Mutex<IpTracker>>,
    logger: ClientLogger,
    socket: TcpStream,
    source_ip: IpAddr,
    is_websocket: bool,
) -> Result<(Sender, Receiver, ForgetClientOnDrop), io::Error> {
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

    let mut decrementer: Option<ForgetClientOnDrop>;
    if is_websocket && get_websocket_proxy_ip().is_some() {
        // Websocket connections should go through nginx and arrive to this process from the proxy ip.
        // The actual client IP is in X-Real-IP header.
        decrementer = None; // created later
    } else {
        // Client connects to rust program directly. Log and limit access with source ip.
        decrementer = Some(IpTracker::track(ip_tracker.clone(), source_ip, logger)?);
    }

    let recv_state = ReceiveState {
        buffer: VecDeque::new(),
        key_press_times: VecDeque::new(),
        last_recv: Instant::now(),
    };

    if is_websocket {
        let config = WebSocketConfig {
            // Prevent various denial-of-service attacks that fill up server's memory.
            // Most defaults are reasonable, but unnecessarily huge for this program.
            max_send_queue: Some(10), // TODO: can be 1? https://github.com/snapview/tungstenite-rs/issues/285
            max_message_size: Some(1024),
            max_frame_size: Some(1024),
            ..Default::default()
        };

        let ws;
        if get_websocket_proxy_ip().is_some() {
            let mut cb = CheckRealIpCallback {
                decrementers: vec![],
                ip_tracker: ip_tracker,
                logger: logger,
            };
            ws = tokio_tungstenite::accept_hdr_async_with_config(socket, &mut cb, Some(config))
                .await
                .map_err(convert_error)?;
            assert!(cb.decrementers.len() == 1);
            decrementer = cb.decrementers.pop();
        } else {
            // Clients connect directly to server, source ip is usable
            ws = tokio_tungstenite::accept_async_with_config(socket, Some(config))
                .await
                .map_err(convert_error)?;
        }

        assert!(decrementer.is_some());

        let (ws_writer, ws_reader) = ws.split();
        sender = Sender::WebSocket { ws_writer };
        receiver = Receiver::WebSocket {
            ws_reader,
            recv_state,
        };
    } else {
        let (read_half, write_half) = socket.into_split();
        sender = Sender::RawTcp { write_half };
        receiver = Receiver::RawTcp {
            read_half,
            recv_state,
        };
    }

    Ok((sender, receiver, decrementer.unwrap()))
}
