//! Native G0 messages over mutually authenticated TLS 1.3. No HTTP, implicit
//! roots, certificate-verification bypass, early data or automatic retries.
use crate::{
    gir::{Capability, CapabilityClass, SemanticType},
    value::Value,
    value_codec::{CodecError, CodecLimits, decode_value, encode_value},
};
use rustls::{
    ClientConfig, ClientConnection, RootCertStore, ServerConfig, ServerConnection, StreamOwned,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName},
};
use std::{
    collections::BTreeSet,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::Arc,
    time::{Duration, Instant},
};

const ALPN: &[u8] = b"g0.native.v1";
#[derive(Debug)]
pub enum TransportError {
    Denied,
    Closed,
    Limit,
    Protocol,
    Sequence,
    Identity,
    Tls(rustls::Error),
    Io(std::io::Error),
    Codec(CodecError),
}
impl From<std::io::Error> for TransportError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rustls::Error> for TransportError {
    fn from(e: rustls::Error) -> Self {
        Self::Tls(e)
    }
}
impl From<CodecError> for TransportError {
    fn from(e: CodecError) -> Self {
        Self::Codec(e)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct TransportLimits {
    pub timeout: Duration,
    pub max_frame_bytes: usize,
    pub max_value_bytes: usize,
    pub max_messages: u64,
}
impl Default for TransportLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            max_frame_bytes: 1024 * 1024,
            max_value_bytes: 8 * 1024 * 1024,
            max_messages: 1_000_000,
        }
    }
}
impl TransportLimits {
    fn validate(self) -> Result<Self, TransportError> {
        if self.timeout.is_zero()
            || self.timeout > Duration::from_secs(60)
            || self.max_frame_bytes == 0
            || self.max_frame_bytes > 64 * 1024 * 1024
            || self.max_value_bytes == 0
            || self.max_value_bytes > 256 * 1024 * 1024
            || self.max_messages == 0
        {
            return Err(TransportError::Limit);
        }
        Ok(self)
    }
}

pub struct Identity {
    certificates: Vec<CertificateDer<'static>>,
    key: PrivatePkcs8KeyDer<'static>,
}
impl Identity {
    pub fn new(certificates: Vec<Vec<u8>>, key: Vec<u8>) -> Result<Self, TransportError> {
        if certificates.is_empty()
            || certificates.len() > 8
            || certificates
                .iter()
                .any(|v| v.is_empty() || v.len() > 256 * 1024)
            || key.is_empty()
            || key.len() > 64 * 1024
        {
            return Err(TransportError::Identity);
        }
        Ok(Self {
            certificates: certificates.into_iter().map(CertificateDer::from).collect(),
            key: key.into(),
        })
    }
}
pub struct NetworkPermit {
    action: String,
    address: SocketAddr,
}
impl NetworkPermit {
    pub fn authorize(
        action: &str,
        address: SocketAddr,
        grants: &BTreeSet<Capability>,
    ) -> Result<Self, TransportError> {
        if !matches!(action, "connect" | "listen")
            || !grants.contains(&Capability::new(
                CapabilityClass::Network,
                action,
                address.to_string(),
                "transport",
            ))
            || !grants.contains(&Capability::new(
                CapabilityClass::Entropy,
                "generate",
                "tls-entropy",
                "transport",
            ))
        {
            return Err(TransportError::Denied);
        }
        Ok(Self {
            action: action.into(),
            address,
        })
    }
}

fn roots(certificates: Vec<Vec<u8>>) -> Result<RootCertStore, TransportError> {
    if certificates.is_empty() || certificates.len() > 256 {
        return Err(TransportError::Identity);
    }
    let mut roots = RootCertStore::empty();
    for cert in certificates {
        if cert.len() > 256 * 1024 {
            return Err(TransportError::Limit);
        }
        roots.add(cert.into())?;
    }
    Ok(roots)
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    pub certificate_sha256: [u8; 32],
}
fn peer(
    certificates: Option<&[CertificateDer<'_>]>,
    alpn: Option<&[u8]>,
) -> Result<PeerIdentity, TransportError> {
    if alpn != Some(ALPN) {
        return Err(TransportError::Protocol);
    }
    let certificate = certificates
        .and_then(|c| c.first())
        .ok_or(TransportError::Identity)?;
    Ok(PeerIdentity {
        certificate_sha256: ring::digest::digest(&ring::digest::SHA256, certificate.as_ref())
            .as_ref()
            .try_into()
            .unwrap(),
    })
}

pub struct SecureListener {
    socket: TcpListener,
    config: Arc<ServerConfig>,
    limits: TransportLimits,
}
impl SecureListener {
    pub fn bind(
        permit: NetworkPermit,
        identity: Identity,
        trusted_client_roots: Vec<Vec<u8>>,
        limits: TransportLimits,
    ) -> Result<Self, TransportError> {
        if permit.action != "listen" {
            return Err(TransportError::Denied);
        }
        let limits = limits.validate()?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
            Arc::new(roots(trusted_client_roots)?),
            provider.clone(),
        )
        .build()
        .map_err(|_| TransportError::Identity)?;
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(identity.certificates, identity.key.into())?;
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.max_early_data_size = 0;
        config.send_tls13_tickets = 0;
        let socket = TcpListener::bind(permit.address)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            config: Arc::new(config),
            limits,
        })
    }
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        Ok(self.socket.local_addr()?)
    }
    pub fn accept(&self) -> Result<SecureChannel, TransportError> {
        let deadline = Instant::now() + self.limits.timeout;
        let socket = loop {
            if Instant::now() >= deadline {
                return Err(TransportError::Io(std::io::ErrorKind::TimedOut.into()));
            }
            match self.socket.accept() {
                Ok((socket, _)) => break socket,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) => return Err(e.into()),
            }
        };
        socket.set_nonblocking(false)?;
        socket.set_nodelay(true)?;
        let mut socket = DeadlineSocket::new(socket, self.limits.timeout);
        let mut conn = ServerConnection::new(self.config.clone())?;
        while conn.is_handshaking() {
            conn.complete_io(&mut socket)?;
        }
        let peer = peer(conn.peer_certificates(), conn.alpn_protocol())?;
        Ok(SecureChannel {
            wire: Wire::Server(StreamOwned::new(conn, socket)),
            peer,
            limits: self.limits,
            sent: 0,
            received: 0,
            closed: false,
        })
    }
}

struct DeadlineSocket {
    socket: TcpStream,
    timeout: Duration,
    deadline: Instant,
}
impl DeadlineSocket {
    fn new(socket: TcpStream, timeout: Duration) -> Self {
        Self {
            socket,
            timeout,
            deadline: Instant::now() + timeout,
        }
    }
    fn reset(&mut self) {
        self.deadline = Instant::now() + self.timeout;
    }
    fn remaining(&self) -> std::io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| std::io::ErrorKind::TimedOut.into())
    }
}
impl Read for DeadlineSocket {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.socket.set_read_timeout(Some(self.remaining()?))?;
        self.socket.read(buf)
    }
}
impl Write for DeadlineSocket {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.socket.set_write_timeout(Some(self.remaining()?))?;
        self.socket.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}
enum Wire {
    Client(StreamOwned<ClientConnection, DeadlineSocket>),
    Server(StreamOwned<ServerConnection, DeadlineSocket>),
}
impl Wire {
    fn reset(&mut self) {
        match self {
            Self::Client(s) => s.sock.reset(),
            Self::Server(s) => s.sock.reset(),
        }
    }
}
impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Client(s) => s.read(buf),
            Self::Server(s) => s.read(buf),
        }
    }
}
impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Client(s) => s.write(buf),
            Self::Server(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Client(s) => s.flush(),
            Self::Server(s) => s.flush(),
        }
    }
}

pub struct SecureChannel {
    wire: Wire,
    peer: PeerIdentity,
    limits: TransportLimits,
    sent: u64,
    received: u64,
    closed: bool,
}
impl SecureChannel {
    pub fn connect(
        permit: NetworkPermit,
        server_name: String,
        identity: Identity,
        trusted_server_roots: Vec<Vec<u8>>,
        limits: TransportLimits,
    ) -> Result<Self, TransportError> {
        if permit.action != "connect" {
            return Err(TransportError::Denied);
        }
        let limits = limits.validate()?;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(roots(trusted_server_roots)?)
            .with_client_auth_cert(identity.certificates, identity.key.into())?;
        config.alpn_protocols = vec![ALPN.to_vec()];
        config.enable_early_data = false;
        config.resumption = rustls::client::Resumption::disabled();
        let name = ServerName::try_from(server_name).map_err(|_| TransportError::Identity)?;
        let mut conn = ClientConnection::new(Arc::new(config), name)?;
        let socket = TcpStream::connect_timeout(&permit.address, limits.timeout)?;
        socket.set_nodelay(true)?;
        let mut socket = DeadlineSocket::new(socket, limits.timeout);
        while conn.is_handshaking() {
            conn.complete_io(&mut socket)?;
        }
        let peer = peer(conn.peer_certificates(), conn.alpn_protocol())?;
        Ok(Self {
            wire: Wire::Client(StreamOwned::new(conn, socket)),
            peer,
            limits,
            sent: 0,
            received: 0,
            closed: false,
        })
    }
    pub fn peer_identity(&self) -> &PeerIdentity {
        &self.peer
    }
    pub fn send(
        &mut self,
        value: &Value,
        ty: &SemanticType,
        schemas: &[crate::data_format::DataSchema],
    ) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        if self.sent >= self.limits.max_messages {
            return Err(TransportError::Limit);
        }
        if value
            .resident_bytes()
            .is_none_or(|b| b > self.limits.max_value_bytes as u64 / 2)
        {
            return Err(TransportError::Limit);
        }
        let bytes = encode_value(
            value,
            ty,
            schemas,
            CodecLimits {
                max_bytes: self.limits.max_frame_bytes,
                ..Default::default()
            },
        )?;
        let header = header(self.sent, bytes.len(), self.limits.max_frame_bytes)?;
        self.wire.reset();
        if let Err(e) = self
            .wire
            .write_all(&header)
            .and_then(|_| self.wire.write_all(&bytes))
            .and_then(|_| self.wire.flush())
        {
            self.closed = true;
            return Err(e.into());
        }
        self.sent = self.sent.checked_add(1).ok_or(TransportError::Sequence)?;
        Ok(())
    }
    pub fn receive(
        &mut self,
        ty: &SemanticType,
        schemas: &[crate::data_format::DataSchema],
    ) -> Result<Value, TransportError> {
        if self.closed {
            return Err(TransportError::Closed);
        }
        if self.received >= self.limits.max_messages {
            return Err(TransportError::Limit);
        }
        self.wire.reset();
        let result = (|| {
            let mut header = [0; 16];
            self.wire.read_exact(&mut header)?;
            let length = parse_header(&header, self.received, self.limits.max_frame_bytes)?;
            let mut bytes = vec![0; length];
            self.wire.read_exact(&mut bytes)?;
            Ok(decode_value(
                &bytes,
                ty,
                schemas,
                CodecLimits {
                    max_bytes: self.limits.max_value_bytes,
                    ..Default::default()
                },
            )?)
        })();
        match result {
            Ok(value) => {
                self.received = self
                    .received
                    .checked_add(1)
                    .ok_or(TransportError::Sequence)?;
                Ok(value)
            }
            Err(e) => {
                self.closed = true;
                Err(e)
            }
        }
    }
}

fn header(sequence: u64, length: usize, max: usize) -> Result<[u8; 16], TransportError> {
    if length > max {
        return Err(TransportError::Limit);
    }
    let mut header = [0; 16];
    header[..4].copy_from_slice(b"G0N\0");
    header[4..12].copy_from_slice(&sequence.to_le_bytes());
    header[12..].copy_from_slice(
        &u32::try_from(length)
            .map_err(|_| TransportError::Limit)?
            .to_le_bytes(),
    );
    Ok(header)
}
fn parse_header(header: &[u8; 16], expected: u64, max: usize) -> Result<usize, TransportError> {
    if &header[..4] != b"G0N\0" {
        return Err(TransportError::Protocol);
    }
    if u64::from_le_bytes(header[4..12].try_into().unwrap()) != expected {
        return Err(TransportError::Sequence);
    }
    let length = usize::try_from(u32::from_le_bytes(header[12..].try_into().unwrap()))
        .map_err(|_| TransportError::Limit)?;
    if length > max {
        return Err(TransportError::Limit);
    }
    Ok(length)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_order_and_lengths_are_rejected_before_allocation() {
        let frame = header(0, 42, 100).unwrap();
        assert_eq!(parse_header(&frame, 0, 100).unwrap(), 42);
        assert!(matches!(
            parse_header(&frame, 1, 100),
            Err(TransportError::Sequence)
        ));
        assert!(matches!(
            parse_header(&frame, 0, 41),
            Err(TransportError::Limit)
        ));
        let mut bad = frame;
        bad[0] = 255;
        assert!(matches!(
            parse_header(&bad, 0, 100),
            Err(TransportError::Protocol)
        ));
        let mut bad = frame;
        bad[12..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            parse_header(&bad, 0, 100),
            Err(TransportError::Limit)
        ));
    }
}
