//! Network DATs: UDP in and out.
//!
//! PLAN.md §5 Phase 3 lists network DATs alongside the core set. UDP is the
//! one worth having first: it is what lighting desks, show controllers and
//! `netcat` all speak with no session to manage, and it follows the same
//! rules as every device in this codebase — the socket lives on its own
//! thread, hands finished messages to the cook, and a failure is a note on
//! the node rather than a failed frame.

use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Persistent socket state, owned by the DAT engine and keyed by node path,
/// exactly as the CHOP `Io` does it.
#[derive(Default)]
pub struct Net {
    pub(crate) udp_in: HashMap<String, UdpIn>,
    pub(crate) udp_out: HashMap<String, UdpOut>,
}

impl Net {
    pub fn reset(&mut self) {
        self.udp_in.clear();
        self.udp_out.clear();
    }
}

/// How many received messages are kept if the patch never reads any away.
pub(crate) const KEEP_CAP: usize = 1000;

pub(crate) struct UdpIn {
    pub messages: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    pub port: u16,
}

impl Drop for UdpIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl UdpIn {
    pub fn open(port: u16) -> Result<UdpIn, String> {
        let socket = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port)))
            .map_err(|e| format!("UDP port {port}: {e}"))?;
        socket
            .set_read_timeout(Some(std::time::Duration::from_millis(200)))
            .map_err(|e| e.to_string())?;

        let messages: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = messages.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        std::thread::Builder::new()
            .name(format!("otd-udp-in-{port}"))
            .spawn(move || {
                let mut buf = [0u8; 65536];
                while !stop_flag.load(Ordering::Relaxed) {
                    let Ok((len, _)) = socket.recv_from(&mut buf) else {
                        continue;
                    };
                    // Lossy rather than rejecting: a DAT is text, and half a
                    // message you can read beats a message you never see.
                    let text = String::from_utf8_lossy(&buf[..len])
                        .trim_end_matches(['\r', '\n'])
                        .to_string();
                    let Ok(mut messages) = sink.lock() else { break };
                    messages.push(text);
                    if messages.len() > KEEP_CAP {
                        let excess = messages.len() - KEEP_CAP;
                        messages.drain(..excess);
                    }
                }
            })
            .map_err(|e| e.to_string())?;

        Ok(UdpIn {
            messages,
            stop,
            port,
        })
    }
}

pub(crate) struct UdpOut {
    pub socket: UdpSocket,
    pub target: SocketAddr,
    /// What was last put on the wire. A DAT cooks whenever anything upstream
    /// does; resending an unchanged payload every cook would turn a cook
    /// into a broadcast.
    pub sent: Option<String>,
}

impl UdpOut {
    pub fn open(target: SocketAddr) -> Result<UdpOut, String> {
        let socket =
            UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], 0))).map_err(|e| e.to_string())?;
        Ok(UdpOut {
            socket,
            target,
            sent: None,
        })
    }
}
