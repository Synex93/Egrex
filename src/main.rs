use anyhow::{Context, Result, bail};
use fast_socks5::{
    ReplyError, Socks5Command,
    server::{Socks5ServerProtocol, transfer},
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const LISTEN_ADDR: &str = "127.0.0.1:1080";
const UPSTREAM_ADDR: &str = "127.0.0.1:10808";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let listener = TcpListener::bind(LISTEN_ADDR)
        .await
        .with_context(|| format!("failed to bind local socks5 proxy on {LISTEN_ADDR}"))?;

    tracing::info!(
        listen = LISTEN_ADDR,
        upstream = UPSTREAM_ADDR,
        "socks5 proxy started"
    );

    loop {
        let (socket, peer_addr) = listener.accept().await?;

        tokio::spawn(async move {
            if let Err(err) = handle_client(socket, peer_addr).await {
                tracing::warn!(client = %peer_addr, error = %err, "socks5 request failed");
            }
        });
    }
}

async fn handle_client(socket: TcpStream, peer_addr: SocketAddr) -> Result<()> {
    let (proto, command, target_addr) = Socks5ServerProtocol::accept_no_auth(socket)
        .await
        .context("failed to accept socks5 no-auth handshake")?
        .read_command()
        .await
        .context("failed to read socks5 command")?;

    if command != Socks5Command::TCPConnect {
        proto
            .reply_error(&ReplyError::CommandNotSupported)
            .await
            .context("failed to reply unsupported command")?;
        bail!("unsupported socks5 command: {command:?}");
    }

    let (target_host, target_port) = target_addr.into_string_and_port();

    tracing::info!(
        client = %peer_addr,
        target = %format!("{target_host}:{target_port}"),
        upstream = UPSTREAM_ADDR,
        "proxying through upstream socks5"
    );

    let upstream = connect_via_upstream_socks5(UPSTREAM_ADDR, &target_host, target_port)
        .await
        .context("failed to connect target through upstream socks5")?;

    let client = proto
        .reply_success(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .context("failed to reply socks5 success")?;

    transfer(client, upstream).await;
    Ok(())
}

async fn connect_via_upstream_socks5(
    upstream_addr: &str,
    target_host: &str,
    target_port: u16,
) -> Result<TcpStream> {
    let upstream_addr = upstream_addr
        .to_socket_addrs()
        .context("invalid upstream socks5 address")?
        .next()
        .context("upstream socks5 address resolved to nothing")?;

    let mut stream = TcpStream::connect(upstream_addr)
        .await
        .context("failed to connect upstream socks5 tcp socket")?;

    let auth_request = [0x05, 0x01, 0x00];
    tracing::info!(bytes = %hex(&auth_request), "writing upstream socks5 auth request");
    stream
        .write_all(&auth_request)
        .await
        .context("failed to write upstream socks5 auth methods")?;

    let mut auth_reply = [0_u8; 2];
    stream
        .read_exact(&mut auth_reply)
        .await
        .context("failed to read upstream socks5 auth reply")?;
    tracing::info!(bytes = %hex(&auth_reply), "read upstream socks5 auth reply");

    if auth_reply != [0x05, 0x00] {
        bail!("upstream socks5 rejected no-auth method: {auth_reply:?}");
    }

    let request = build_socks5_connect_request(target_host, target_port)?;
    tracing::info!(bytes = %hex(&request), "writing upstream socks5 connect request");
    stream
        .write_all(&request)
        .await
        .context("failed to write upstream socks5 connect request")?;

    read_socks5_connect_reply(&mut stream).await?;
    Ok(stream)
}

fn build_socks5_connect_request(target_host: &str, target_port: u16) -> Result<Vec<u8>> {
    let mut request = vec![0x05, 0x01, 0x00];

    if let Ok(ip) = target_host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(ip) => {
                request.push(0x01);
                request.extend_from_slice(&ip.octets());
            }
            IpAddr::V6(ip) => {
                request.push(0x04);
                request.extend_from_slice(&ip.octets());
            }
        }
    } else {
        let host = target_host.as_bytes();
        if host.len() > u8::MAX as usize {
            bail!("target domain is too long for socks5: {target_host}");
        }

        request.push(0x03);
        request.push(host.len() as u8);
        request.extend_from_slice(host);
    }

    request.extend_from_slice(&target_port.to_be_bytes());
    Ok(request)
}

async fn read_socks5_connect_reply(stream: &mut TcpStream) -> Result<()> {
    let mut header = [0_u8; 4];
    stream
        .read_exact(&mut header)
        .await
        .context("failed to read upstream socks5 connect reply header")?;
    tracing::info!(bytes = %hex(&header), "read upstream socks5 connect reply header");

    if header[0] != 0x05 {
        bail!("invalid upstream socks5 reply version: {}", header[0]);
    }

    if header[1] != 0x00 {
        bail!(
            "upstream socks5 connect failed with reply code: {}",
            header[1]
        );
    }

    let address_len = match header[3] {
        0x01 => 4,
        0x03 => {
            let mut len = [0_u8; 1];
            stream
                .read_exact(&mut len)
                .await
                .context("failed to read upstream socks5 domain reply length")?;
            len[0] as usize
        }
        0x04 => 16,
        atyp => bail!("unsupported upstream socks5 reply address type: {atyp}"),
    };

    let mut rest = vec![0_u8; address_len + 2];
    stream
        .read_exact(&mut rest)
        .await
        .context("failed to read upstream socks5 connect reply address")?;

    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}
