use crate::ClientLogger;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::io;
use std::io::ErrorKind;
use std::net::IpAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

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
