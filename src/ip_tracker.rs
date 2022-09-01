use crate::ClientLogger;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::env;
use std::io;
use std::io::ErrorKind;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;
use tokio_tungstenite::tungstenite::handshake::server::Callback;
use tokio_tungstenite::tungstenite::handshake::server::ErrorResponse;
use tokio_tungstenite::tungstenite::handshake::server::Request;
use tokio_tungstenite::tungstenite::handshake::server::Response;

pub fn websocket_connections_come_from_a_proxy() -> bool {
    return !env::var("CATRIS_WEBSOCKET_PROXIED")
        .unwrap_or("".to_string())
        .is_empty();
}

pub struct IpTracker {
    recent_ips: VecDeque<(Instant, IpAddr)>,
    client_counts_by_ip: HashMap<IpAddr, usize>,
}

pub struct ForgetClientOnDrop {
    logger: ClientLogger,
    ip: IpAddr,
    ip_tracker: Arc<Mutex<IpTracker>>,
}
impl Drop for ForgetClientOnDrop {
    fn drop(&mut self) {
        let mut tracker = self.ip_tracker.lock().unwrap();
        let n = *tracker.client_counts_by_ip.get(&self.ip).unwrap();
        assert!(n > 0);
        if n == 1 {
            // last client
            _ = tracker.client_counts_by_ip.remove(&self.ip).unwrap();
        } else {
            tracker.client_counts_by_ip.insert(self.ip, n - 1);
        }

        let total: usize = tracker.client_counts_by_ip.values().sum();
        self.logger
            .log(&format!("There are now {} connected clients", total));
    }
}

fn parse_real_ip_header(x_real_ip: Option<&str>) -> Result<IpAddr, io::Error> {
    if x_real_ip.is_none() {
        return Err(io::Error::new(
            ErrorKind::ConnectionAborted,
            "missing X-Real-IP header",
        ));
    }

    // nginx sets a header to indicate what the user's actual IP is.
    // If it's missing, nginx is most likely misconfigured and we don't want to e.g. default to localhost (#267)
    let ip: IpAddr = x_real_ip.unwrap().parse().map_err(|_| {
        io::Error::new(
            ErrorKind::ConnectionAborted,
            "cannot parse X-Real-IP header",
        )
    })?;
    Ok(ip)
}

pub struct CheckProxyIpCallback {
    logger: ClientLogger,
    ip_tracker: Arc<Mutex<IpTracker>>,
    pub result_if_called: Option<Result<ForgetClientOnDrop, io::Error>>,
}
impl CheckProxyIpCallback {
    pub fn new(logger: ClientLogger, ip_tracker: Arc<Mutex<IpTracker>>) -> Self {
        return Self {
            logger,
            ip_tracker,
            result_if_called: None,
        };
    }
}
impl Callback for &mut CheckProxyIpCallback {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        assert!(self.result_if_called.is_none()); // not called twice
        let x_real_ip: Option<&str> = request
            .headers()
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok());

        let result = parse_real_ip_header(x_real_ip).map(|ip| ForgetClientOnDrop {
            logger: self.logger,
            ip_tracker: self.ip_tracker.clone(),
            ip,
        });
        self.result_if_called = Some(result);

        // All this was just to get the ip from a header. Keep going as usual...
        Ok(response)
    }
}

impl IpTracker {
    pub fn new() -> Self {
        Self {
            recent_ips: VecDeque::new(),
            client_counts_by_ip: HashMap::new(),
        }
    }

    pub fn track(
        tracker_arcmutex: Arc<Mutex<IpTracker>>,
        ip: IpAddr,
        logger: ClientLogger,
    ) -> Result<ForgetClientOnDrop, io::Error> {
        {
            let mut tracker = tracker_arcmutex.lock().unwrap();
            tracker.recent_ips.push_back((Instant::now(), ip));
            while !tracker.recent_ips.is_empty()
                && tracker.recent_ips[0].0.elapsed().as_secs_f32() > 60.0
            {
                tracker.recent_ips.pop_front();
            }

            let n = tracker
                .recent_ips
                .iter()
                .filter(|(_, recent_ip)| *recent_ip == ip)
                .count();
            if n >= 5 {
                logger.log(&format!(
                    "This is the {}th connection from IP address {} within the last minute",
                    n, ip
                ));
            }

            let old_count = *tracker.client_counts_by_ip.get(&ip).unwrap_or(&0);
            if old_count >= 5 {
                return Err(io::Error::new(
                    ErrorKind::ConnectionAborted,
                    format!(
                        "there are already {} other connections from the same IP",
                        old_count
                    ),
                ));
            }
            tracker.client_counts_by_ip.insert(ip, old_count + 1);

            let total: usize = tracker.client_counts_by_ip.values().sum();
            logger.log(&format!("There are now {} connected clients", total));
        }

        Ok(ForgetClientOnDrop {
            ip,
            ip_tracker: tracker_arcmutex,
            logger,
        })
    }
}
