use crate::client::log_for_client;
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
    client_id: u64,
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
            tracker.client_counts_by_ip.remove(&self.ip).unwrap();
        } else {
            tracker.client_counts_by_ip.insert(self.ip, n - 1);
        }

        let total: usize = tracker.client_counts_by_ip.values().sum();
        log_for_client(
            self.client_id,
            &format!("There are now {} connected clients", total),
        );
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
        client_id: u64,
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
                log_for_client(
                    client_id,
                    &format!(
                        "This is the {}th connection from IP address {} within the last minute",
                        n, ip
                    ),
                );
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
            log_for_client(
                client_id,
                &format!("There are now {} connected clients", total),
            );
        }

        Ok(ForgetClientOnDrop {
            ip,
            ip_tracker: tracker_arcmutex,
            client_id,
        })
    }
}
