use anyhow::{Context, Result, bail};
use fast_socks5::{ReplyError, Socks5Command, server::Socks5ServerProtocol};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs},
    path::PathBuf,
    sync::Arc,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, RwLock},
    time::{self, Duration},
};

use crate::{config::AppConfig, daemon, pool::ProxyPool, traffic::TrafficCounter};

pub async fn run(
    config: Arc<RwLock<AppConfig>>,
    config_path: impl Into<PathBuf>,
    traffic: Arc<TrafficCounter>,
    pool: ProxyPool,
    pid_lock_path: impl Into<PathBuf>,
    stop_path: impl Into<PathBuf>,
) -> Result<()> {
    let config_path = config_path.into();
    let pid_lock_path = pid_lock_path.into();
    let mut current_config = config.read().await.clone();
    let mut listen_addr = current_config.listen_addr();
    let stop_path = stop_path.into();
    daemon::clear_shutdown_request(&stop_path)?;

    let mut listener = bind_listener(&listen_addr).await?;
    let active_connections = Arc::new(ActiveConnections::default());

    tracing::info!(listen = %listen_addr, "socks5 proxy started");

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (socket, peer_addr) = result?;
                let traffic = traffic.clone();
                let pool = pool.clone();
                let request_config = config.read().await.clone();
                let check_url = request_config.check_url;
                let max_latency = Duration::from_millis(request_config.max_latency.max(1));
                let active_connections = active_connections.clone();
                let _guard = active_connections.guard();

                tokio::spawn(async move {
                    let _guard = _guard;
                    if let Err(err) = handle_client(socket, peer_addr, traffic, pool, check_url, max_latency).await {
                        tracing::warn!(client = %peer_addr, error = %err, "socks5 request failed");
                    }
                });
            }
            _ = time::sleep(Duration::from_millis(500)) => {
                if daemon::shutdown_requested(&stop_path) {
                    tracing::info!("stop request received, waiting for active connections");
                    active_connections.wait_until_empty().await;
                    daemon::clear_shutdown_request(&stop_path)?;
                    tracing::info!("socks5 proxy stopped gracefully");
                    return Ok(());
                }

                match AppConfig::load_or_default(&config_path) {
                    Ok(next_config) if next_config != current_config => {
                        let next_listen_addr = next_config.listen_addr();
                        {
                            let mut config = config.write().await;
                            *config = next_config.clone();
                        }

                        if next_listen_addr != listen_addr {
                            listener = bind_listener(&next_listen_addr).await?;
                            daemon::update_listen_addr(&pid_lock_path, next_listen_addr.clone())?;
                            tracing::info!(old_listen = %listen_addr, listen = %next_listen_addr, "socks5 listener reloaded");
                            listen_addr = next_listen_addr;
                        }

                        current_config = next_config;
                    }
                    Ok(_) => {}
                    Err(err) => tracing::warn!(error = %err, "failed to reload config"),
                }
            }
        }
    }
}

async fn bind_listener(listen_addr: &str) -> Result<TcpListener> {
    TcpListener::bind(listen_addr)
        .await
        .with_context(|| format!("failed to bind local socks5 proxy on {listen_addr}"))
}

#[derive(Debug, Default)]
struct ActiveConnections {
    count: std::sync::atomic::AtomicUsize,
    notify: Notify,
}

impl ActiveConnections {
    fn guard(self: &Arc<Self>) -> ActiveConnectionGuard {
        self.count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ActiveConnectionGuard {
            active_connections: self.clone(),
        }
    }

    async fn wait_until_empty(&self) {
        loop {
            if self.count.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                return;
            }

            self.notify.notified().await;
        }
    }
}

#[derive(Debug)]
struct ActiveConnectionGuard {
    active_connections: Arc<ActiveConnections>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        if self
            .active_connections
            .count
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed)
            == 1
        {
            self.active_connections.notify.notify_waiters();
        }
    }
}

async fn handle_client(
    socket: TcpStream,
    peer_addr: SocketAddr,
    traffic: Arc<TrafficCounter>,
    pool: ProxyPool,
    check_url: String,
    max_latency: Duration,
) -> Result<()> {
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

    let upstream_addr = pool
        .select_upstream(&check_url, max_latency)
        .await?
        .context("online upstream pool is empty")?;

    tracing::info!(client = %peer_addr, target = %format!("{target_host}:{target_port}"), upstream = %upstream_addr, "proxying through upstream socks5");

    let upstream =
        match connect_via_upstream_socks5(&upstream_addr, &target_host, target_port).await {
            Ok(upstream) => upstream,
            Err(err) => {
                pool.mark_failure(&upstream_addr).await?;
                return Err(err).context("failed to connect target through upstream socks5");
            }
        };

    let mut client = proto
        .reply_success(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .context("failed to reply socks5 success")?;
    let mut upstream = upstream;

    let (upload_bytes, download_bytes) =
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    traffic.add(upload_bytes, download_bytes)?;
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
