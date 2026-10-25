//! Transports move datagrams between peers. The lockstep protocol only needs
//! unreliable, unordered datagrams: it repeats whatever hasn't been
//! acknowledged. Implementations: [`UdpTransport`] (LAN, Tailscale, direct
//! IP), [`NullTransport`] (single-player), and [`crate::netsim`] for tests.
//! Steam and EOS come later behind the same trait.

use std::fmt::Debug;
use std::hash::Hash;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};

pub trait Transport {
    type Addr: Clone + Eq + Ord + Hash + Debug;

    /// Sends one datagram. Delivery is not guaranteed.
    fn send(&mut self, to: &Self::Addr, bytes: &[u8]);

    /// The next datagram received, if any. Never blocks.
    fn recv(&mut self) -> Option<(Self::Addr, Vec<u8>)>;
}

impl<A: Clone + Eq + Ord + Hash + Debug> Transport for Box<dyn Transport<Addr = A> + Send + Sync> {
    type Addr = A;

    fn send(&mut self, to: &A, bytes: &[u8]) {
        (**self).send(to, bytes)
    }

    fn recv(&mut self) -> Option<(A, Vec<u8>)> {
        (**self).recv()
    }
}

/// Plain UDP. Our own framing (see `protocol`) rides on top.
pub struct UdpTransport {
    socket: UdpSocket,
    buffer: Vec<u8>,
}

impl UdpTransport {
    /// Binds to `addr`, for example `0.0.0.0:7777` to host or `0.0.0.0:0` to join.
    pub fn bind(addr: SocketAddr) -> std::io::Result<UdpTransport> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        Ok(UdpTransport { socket, buffer: vec![0; 64 * 1024] })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}

impl Transport for UdpTransport {
    type Addr = SocketAddr;

    fn send(&mut self, to: &SocketAddr, bytes: &[u8]) {
        // Errors (full buffers, unreachable hosts) are packet loss to the protocol.
        let _ = self.socket.send_to(bytes, to);
    }

    fn recv(&mut self) -> Option<(SocketAddr, Vec<u8>)> {
        loop {
            match self.socket.recv_from(&mut self.buffer) {
                Ok((len, from)) => return Some((from, self.buffer[..len].to_vec())),
                Err(e) if e.kind() == ErrorKind::WouldBlock => return None,
                // Windows reports an earlier send's ICMP "port unreachable" on
                // the next receive. It says nothing about this datagram.
                Err(e) if e.kind() == ErrorKind::ConnectionReset => continue,
                Err(_) => return None,
            }
        }
    }
}

/// Sends nothing and receives nothing: a host with no clients.
pub struct NullTransport;

impl Transport for NullTransport {
    type Addr = SocketAddr;

    fn send(&mut self, _: &SocketAddr, _: &[u8]) {}

    fn recv(&mut self) -> Option<(SocketAddr, Vec<u8>)> {
        None
    }
}
