//! Minimal SOCKS5 server side (RFC 1928): "no authentication" and the CONNECT
//! command only, which is what browsers and `curl --socks5-hostname` use.
//!
//! The listener only accepts local clients by default, like `ssh -D`: no SOCKS
//! authentication is needed (and OpenSSH offers none either).

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const VERSION: u8 = 5;
const NO_AUTH: u8 = 0;
const NO_ACCEPTABLE_METHOD: u8 = 0xFF;
const CMD_CONNECT: u8 = 1;
const ATYP_IPV4: u8 = 1;
const ATYP_DOMAIN: u8 = 3;
const ATYP_IPV6: u8 = 4;

/// Reply codes sent back to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Succeeded = 0,
    ConnectionRefused = 5,
    CommandNotSupported = 7,
    AddressTypeNotSupported = 8,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SocksError {
    /// Not SOCKS5 (SOCKS4, HTTP proxy request...).
    BadVersion(u8),
    /// The client does not offer "no authentication".
    NoAcceptableMethod,
    /// BIND / UDP ASSOCIATE: only CONNECT is supported.
    UnsupportedCommand(u8),
    UnsupportedAddressType(u8),
    Io(String),
}

impl std::fmt::Display for SocksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SocksError::BadVersion(v) => {
                write!(f, "client SOCKS version {v} (seul SOCKS5 est géré)")
            }
            SocksError::NoAcceptableMethod => {
                write!(f, "le client SOCKS exige une authentification non gérée")
            }
            SocksError::UnsupportedCommand(c) => {
                write!(f, "commande SOCKS {c} non gérée (seul CONNECT l'est)")
            }
            SocksError::UnsupportedAddressType(t) => write!(f, "type d'adresse SOCKS {t} inconnu"),
            SocksError::Io(e) => write!(f, "client SOCKS : {e}"),
        }
    }
}

impl From<std::io::Error> for SocksError {
    fn from(e: std::io::Error) -> Self {
        SocksError::Io(e.to_string())
    }
}

/// Reads the greeting and the CONNECT request; returns the requested host and
/// port. On a protocol error the matching failure reply is already sent.
pub async fn accept<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
) -> Result<(String, u16), SocksError> {
    // Greeting: VER, NMETHODS, METHODS...
    let version = stream.read_u8().await?;
    if version != VERSION {
        return Err(SocksError::BadVersion(version));
    }
    let count = stream.read_u8().await? as usize;
    let mut methods = vec![0u8; count];
    stream.read_exact(&mut methods).await?;
    if !methods.contains(&NO_AUTH) {
        stream.write_all(&[VERSION, NO_ACCEPTABLE_METHOD]).await?;
        return Err(SocksError::NoAcceptableMethod);
    }
    stream.write_all(&[VERSION, NO_AUTH]).await?;

    // Request: VER, CMD, RSV, ATYP, DST.ADDR, DST.PORT
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    let [version, command, _reserved, address_type] = header;
    if version != VERSION {
        return Err(SocksError::BadVersion(version));
    }
    let host = match address_type {
        ATYP_IPV4 => {
            let mut ip = [0u8; 4];
            stream.read_exact(&mut ip).await?;
            std::net::Ipv4Addr::from(ip).to_string()
        }
        ATYP_IPV6 => {
            let mut ip = [0u8; 16];
            stream.read_exact(&mut ip).await?;
            std::net::Ipv6Addr::from(ip).to_string()
        }
        ATYP_DOMAIN => {
            let len = stream.read_u8().await? as usize;
            let mut name = vec![0u8; len];
            stream.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        other => {
            reply(stream, Reply::AddressTypeNotSupported).await?;
            return Err(SocksError::UnsupportedAddressType(other));
        }
    };
    let port = stream.read_u16().await?;
    if command != CMD_CONNECT {
        reply(stream, Reply::CommandNotSupported).await?;
        return Err(SocksError::UnsupportedCommand(command));
    }
    Ok((host, port))
}

/// Final answer to the CONNECT request. The bound address is not meaningful
/// through an SSH tunnel: 0.0.0.0:0, as OpenSSH does.
pub async fn reply<S: AsyncWrite + Unpin>(stream: &mut S, code: Reply) -> std::io::Result<()> {
    stream
        .write_all(&[VERSION, code as u8, 0, ATYP_IPV4, 0, 0, 0, 0, 0, 0])
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// Runs `accept` against `client_bytes`; returns its result and what the
    /// server wrote back.
    async fn run(client_bytes: &[u8]) -> (Result<(String, u16), SocksError>, Vec<u8>) {
        let (mut client, mut server) = duplex(1024);
        client.write_all(client_bytes).await.unwrap();
        let result = accept(&mut server).await;
        drop(server);
        let mut answer = Vec::new();
        client.read_to_end(&mut answer).await.unwrap();
        (result, answer)
    }

    #[tokio::test]
    async fn connect_by_domain_name() {
        // curl --socks5-hostname: the name is resolved by the SSH server.
        let mut bytes = vec![5, 1, 0, 5, 1, 0, 3, 11];
        bytes.extend_from_slice(b"example.lab");
        bytes.extend_from_slice(&443u16.to_be_bytes());
        let (result, answer) = run(&bytes).await;
        assert_eq!(result, Ok(("example.lab".into(), 443)));
        assert_eq!(answer, [5, 0], "only the method choice before the reply");
    }

    #[tokio::test]
    async fn connect_by_ipv4_and_ipv6() {
        let (result, _) = run(&[5, 2, 2, 0, 5, 1, 0, 1, 10, 0, 0, 1, 0, 22]).await;
        assert_eq!(result, Ok(("10.0.0.1".into(), 22)));

        let mut bytes = vec![5, 1, 0, 5, 1, 0, 4];
        bytes.extend_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets());
        bytes.extend_from_slice(&80u16.to_be_bytes());
        let (result, _) = run(&bytes).await;
        assert_eq!(result, Ok(("::1".into(), 80)));
    }

    #[tokio::test]
    async fn refuses_what_is_not_supported() {
        // SOCKS4 request.
        let (result, _) = run(&[4, 1, 0, 80, 10, 0, 0, 1, 0]).await;
        assert_eq!(result, Err(SocksError::BadVersion(4)));

        // Only username/password authentication offered.
        let (result, answer) = run(&[5, 1, 2]).await;
        assert_eq!(result, Err(SocksError::NoAcceptableMethod));
        assert_eq!(answer, [5, 0xFF]);

        // BIND command.
        let (result, answer) = run(&[5, 1, 0, 5, 2, 0, 1, 10, 0, 0, 1, 0, 22]).await;
        assert_eq!(result, Err(SocksError::UnsupportedCommand(2)));
        assert_eq!(answer[2..4], [5, 7], "command not supported reply");
    }
}
