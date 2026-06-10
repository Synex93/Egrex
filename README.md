# Egrex

Egrex is a local SOCKS5 proxy forwarder with an upstream SOCKS5 proxy pool. It listens locally and rotates outbound requests through verified upstream SOCKS5 nodes.

## Features

- Local no-auth SOCKS5 listener.
- Upstream SOCKS5 rotation from the online pool.
- FOFA-based candidate discovery.
- Candidate and online pool maintenance while the service is running.
- Latency-based upstream quality filtering.
- Graceful stop by default, with force stop as an explicit option.
- Runtime config reload for host, port, FOFA settings, check URL, and latency threshold.

## Quick Start

Set the local listener if needed:

```bash
cargo run -- set host 127.0.0.1
cargo run -- set port 1080
```

Set FOFA credentials and API endpoint:

```bash
cargo run -- set fofa-api https://fofa.info/api/v1/
cargo run -- set fofa-key <your_api_key>
```

Set the upstream quality check target and maximum latency:

```bash
cargo run -- set check-url https://cloudflare.com/cdn-cgi/trace
cargo run -- set max-latency 5000
```

Start the proxy in the background:

```bash
cargo run -- start
```

Use this local proxy in your browser, system proxy, curl, or application:

```text
SOCKS5 127.0.0.1:1080
```

Check runtime status:

```bash
cargo run -- status
```

Stop gracefully:

```bash
cargo run -- stop
```

Force stop only when graceful stop cannot finish:

```bash
cargo run -- stop --force
```

## Commands

```bash
cargo run -- run
cargo run -- start
cargo run -- stop
cargo run -- stop --force
cargo run -- status
cargo run -- update
```

Configuration commands:

```bash
cargo run -- set host <host>
cargo run -- set port <port>
cargo run -- set check-url <url>
cargo run -- set max-latency <milliseconds>
cargo run -- set fofa-api <url>
cargo run -- set fofa-key <key>
```

Aliases using underscores are also supported for multi-word settings, for example `fofa_key`, `fofa_api`, `check_url`, and `max_latency`.

## Pool Model

Egrex uses two runtime pools:

```text
candidates.lock  candidate pool, target size 200
online.lock      online pool, target size 80
```

Flow:

```text
FOFA API
  -> candidates.lock
  -> latency and reachability checks
  -> online.lock
  -> local SOCKS5 forwarding
```

While the service is running:

- The candidate pool is automatically refilled from FOFA when it drops below the target size.
- The online pool is checked periodically and replenished from candidates when nodes fail.
- Each new client connection selects an upstream from the online pool in round-robin order.
- Upstreams that fail during forwarding are removed from the online pool.
- Recently checked upstreams are cached briefly to avoid repeated checks.

## Runtime Config Reload

The running service reloads `config.toml` automatically.

Changes to these fields are applied without restarting:

- `host`
- `port`
- `check_url`
- `max_latency`
- `fofa_api`
- `fofa_key`

Changing `host` or `port` causes the listener to rebind to the new address. Existing client connections are allowed to finish naturally.

## Runtime Files

These files are generated locally and are ignored by Git:

```text
config.toml
pid.lock
stop.lock
traffic.lock
upstreams.lock
candidates.lock
online.lock
```

Do not commit `config.toml`; it may contain private FOFA credentials.
