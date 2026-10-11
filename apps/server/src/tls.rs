//! Native origin TLS. Certificate failures never fall back to plaintext.
use anyhow::{bail, Context, Result};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use std::{
    fs::File,
    io::{self, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) struct Identity {
    certificate: PathBuf,
    private_key: PathBuf,
}
impl Identity {
    fn config(&self) -> Result<Arc<ServerConfig>> {
        let mut certificate = self.certificate.clone();
        let mut private_key = self.private_key.clone();
        // Kubernetes projects both files through one atomically swapped ..data
        // directory. Resolve that generation once, so rotation cannot mix keys.
        if certificate.parent() == private_key.parent() {
            if let Some(parent) = certificate.parent() {
                let projected = |path: &Path| {
                    std::fs::read_link(path).is_ok_and(|link| link.starts_with("..data"))
                };
                if projected(&certificate) && projected(&private_key) {
                    let generation = std::fs::canonicalize(parent.join("..data"))?;
                    certificate = generation.join(
                        certificate
                            .file_name()
                            .context("invalid certificate path")?,
                    );
                    private_key =
                        generation.join(private_key.file_name().context("invalid key path")?);
                }
            }
        }
        configure(&certificate, &private_key)
    }
}

pub(super) fn load(
    cert: Option<&str>,
    key: Option<&str>,
    listener: bool,
) -> Result<Option<Arc<Identity>>> {
    let (cert, key) = match (cert, key) {
        (None, None) => return Ok(None),
        (Some(cert), Some(key)) if listener => (cert, key),
        _ => bail!("TLS requires --ws-bind, --tls-cert and --tls-key together"),
    };
    let identity = Identity {
        certificate: cert.into(),
        private_key: key.into(),
    };
    identity.config()?;
    Ok(Some(Arc::new(identity)))
}
fn configure(cert: &Path, key: &Path) -> Result<Arc<ServerConfig>> {
    let chain = rustls_pemfile::certs(&mut BufReader::new(
        File::open(cert).context("cannot open TLS certificate")?,
    ))
    .collect::<io::Result<Vec<_>>>()
    .context("invalid TLS certificate PEM")?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(
        File::open(key).context("cannot open TLS key")?,
    ))?
    .context("TLS PEM contains no private key")?;
    let mut config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()?
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .context("TLS certificate and key are invalid or do not match")?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

// Apply an absolute deadline to every underlying operation, including repeated
// TLS records during a handshake/read. A slow peer cannot renew its timeout.
pub(super) struct TimedSocket {
    socket: TcpStream,
    read_deadline: Instant,
    write_deadline: Instant,
}
fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "transport deadline"))
}
impl Read for TimedSocket {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        self.socket
            .set_read_timeout(Some(remaining(self.read_deadline)?))?;
        self.socket.read(data)
    }
}
impl Write for TimedSocket {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.socket
            .set_write_timeout(Some(remaining(self.write_deadline)?))?;
        self.socket.write(data)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
pub(super) enum Transport {
    Plain(TimedSocket),
    Tls(Box<StreamOwned<ServerConnection, TimedSocket>>),
}
impl Transport {
    pub(super) fn accept(
        socket: TcpStream,
        config: Option<Arc<Identity>>,
        timeout: Duration,
    ) -> Result<Self> {
        let deadline = Instant::now() + timeout;
        let mut socket = TimedSocket {
            socket,
            read_deadline: deadline,
            write_deadline: deadline,
        };
        if let Some(config) = config {
            let mut conn = ServerConnection::new(config.config()?)?;
            while conn.is_handshaking() {
                conn.complete_io(&mut socket)?;
            }
            Ok(Self::Tls(Box::new(StreamOwned::new(conn, socket))))
        } else {
            Ok(Self::Plain(socket))
        }
    }
    fn socket(&mut self) -> &mut TimedSocket {
        match self {
            Self::Plain(s) => s,
            Self::Tls(s) => &mut s.sock,
        }
    }
    pub(super) fn set_read_deadline(&mut self, deadline: Instant) {
        self.socket().read_deadline = deadline;
    }
    pub(super) fn set_write_deadline(&mut self, deadline: Instant) {
        self.socket().write_deadline = deadline;
    }
}
impl Read for Transport {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(data),
            Self::Tls(s) => s.read(data),
        }
    }
}
impl Write for Transport {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(data),
            Self::Tls(s) => s.write(data),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

/// Readiness uses the same verified identity and trust bundle as the connector.
pub(super) fn check_health(address: &str, ca: &str, name: &str) -> Result<()> {
    use std::net::ToSocketAddrs;
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut BufReader::new(File::open(ca)?)) {
        roots.add(cert?)?;
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let identity = rustls::pki_types::ServerName::try_from(name.to_owned())?;
    let address = address
        .to_socket_addrs()?
        .next()
        .context("health address did not resolve")?;
    let timeout = Duration::from_secs(3);
    let socket = TcpStream::connect_timeout(&address, timeout)?;
    let deadline = Instant::now() + timeout;
    let socket = TimedSocket {
        socket,
        read_deadline: deadline,
        write_deadline: deadline,
    };
    let connection = rustls::ClientConnection::new(Arc::new(config), identity)?;
    let mut stream = StreamOwned::new(connection, socket);
    // The request is fixed; identity validation happens in TLS, not a Host header.
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
    let expected = b"{\"simulation_ready\":true}";
    let mut response = Vec::new();
    while response.len() < 1024 {
        let mut bytes = [0; 256];
        let count = stream.read(&mut bytes)?;
        if count == 0 {
            break;
        }
        response.extend_from_slice(&bytes[..count]);
        if let Some(end) = response.windows(4).position(|w| w == b"\r\n\r\n") {
            let body = &response[end + 4..];
            if body.len() >= expected.len() {
                if response.starts_with(b"HTTP/1.1 200 OK\r\n") && body == expected {
                    return Ok(());
                }
                bail!("simulation is not ready");
            }
        }
    }
    bail!("invalid TLS health response")
}
