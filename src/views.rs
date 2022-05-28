use std::io::Write;
use std::net::IpAddr;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::Weak;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::sleep;
use weak_table::WeakValueHashMap;

use crate::render;

pub trait View: Send {
    fn render(&self, buffer: &mut render::RenderBuffer);
}

pub type ViewRef = Arc<Mutex<dyn View>>;

pub struct DummyView {}

impl View for DummyView {
    fn render(&self, buffer: &mut render::RenderBuffer) {
        buffer.resize(0, 0);
    }
}
