# Egrex

[English](README.md) | [简体中文](README.zh-CN.md)

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

Build the binary:

```bash
cargo build --release
```

Windows examples use `target/release/Egrex.exe`. On Linux, use `target/release/Egrex` instead. You can also copy the executable into a directory included in `PATH`.

Set the local listener if needed:

```bash
target/release/Egrex.exe set host 127.0.0.1
target/release/Egrex.exe set port 1080
```

Set FOFA credentials and API endpoint:

```bash
target/release/Egrex.exe set fofa-api https://fofa.info/api/v1/
target/release/Egrex.exe set fofa-key <your_api_key>
```

Set the upstream quality check target and maximum latency:

```bash
target/release/Egrex.exe set check-url https://cloudflare.com/cdn-cgi/trace
target/release/Egrex.exe set max-latency 5000
```

Start the proxy in the background:

```bash
target/release/Egrex.exe start
```

Use this local proxy in your browser, system proxy, curl, or application:

```text
SOCKS5 127.0.0.1:1080
```

Check runtime status:

```bash
target/release/Egrex.exe status
```

Stop gracefully:

```bash
target/release/Egrex.exe stop
```

Force stop only when graceful stop cannot finish:

```bash
target/release/Egrex.exe stop --force
```

## Commands

```bash
target/release/Egrex.exe run
target/release/Egrex.exe start
target/release/Egrex.exe stop
target/release/Egrex.exe stop --force
target/release/Egrex.exe status
target/release/Egrex.exe update
```

Configuration commands:

```bash
target/release/Egrex.exe set host <host>
target/release/Egrex.exe set port <port>
target/release/Egrex.exe set check-url <url>
target/release/Egrex.exe set max-latency <milliseconds>
target/release/Egrex.exe set fofa-api <url>
target/release/Egrex.exe set fofa-key <key>
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

The running service reloads `~/.egrex/config.toml` automatically.

Changes to these fields are applied without restarting:

- `host`
- `port`
- `check_url`
- `max_latency`
- `fofa_api`
- `fofa_key`

Changing `host` or `port` causes the listener to rebind to the new address. Existing client connections are allowed to finish naturally.

## Runtime Files

Runtime files are stored under the user home directory:

```text
~/.egrex
```

These files are generated there and are ignored by Git:

```text
config.toml
pid.lock
stop.lock
traffic.lock
upstreams.lock
candidates.lock
online.lock
```

Do not commit `~/.egrex/config.toml`; it may contain private FOFA credentials.
