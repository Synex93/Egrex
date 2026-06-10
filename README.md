# Egrex

[English](README.md) | [简体中文](README.zh-CN.md)

Egrex is a local SOCKS5 proxy forwarder with an upstream SOCKS5 proxy pool. It listens locally and rotates outbound requests through verified upstream SOCKS5 nodes.

## Features

- Local no-auth SOCKS5 listener.
- Upstream SOCKS5 rotation from the online pool.
- FOFA-based candidate discovery.
- FOFA discovery scans assets discovered within the last 30 days by default.
- FOFA refill stores a page cursor to avoid repeatedly scanning page 1, and waits for the search window to move forward after the current window is exhausted.
- Candidate refill fetches 1000 hosts per page and only refills when the candidate pool drops below 200.
- Candidate and online pool maintenance while the service is running.
- Latency-based upstream quality filtering with two check URLs and retries.
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
target/release/Egrex.exe set check-fallback-url https://myip.ipip.net
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

`update` writes candidates to `~/.egrex/candidates.lock` and updates `~/.egrex/fofa_state.toml` to the fetched page cursor. Use `--page` to reset the cursor manually, and `--limit` to control how many hosts are saved.

Configuration commands:

```bash
target/release/Egrex.exe set host <host>
target/release/Egrex.exe set port <port>
target/release/Egrex.exe set check-url <url>
target/release/Egrex.exe set check-fallback-url <url>
target/release/Egrex.exe set max-latency <milliseconds>
target/release/Egrex.exe set fofa-api <url>
target/release/Egrex.exe set fofa-key <key>
```

Aliases using underscores are also supported for multi-word settings, for example `fofa_key`, `fofa_api`, `check_url`, `check_fallback_url`, and `max_latency`.

## Pool Model

Egrex uses two runtime pools:

```text
candidates.lock  candidate pool, refill when below 200, no fixed upper limit
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
- FOFA pagination continues from `fofa_state.toml`; when a page returns no hosts, refill pauses for that search window instead of restarting at page 1.
- The online pool is checked periodically and replenished from candidates when nodes fail.
- A proxy check tries the primary and fallback check URLs, with three attempts per URL. Any successful URL within `max_latency` keeps the upstream alive.
- Each new client connection selects an upstream from the online pool in round-robin order.
- Upstreams that fail during forwarding are removed from the online pool.
- Recently checked upstreams are cached briefly to avoid repeated checks.

## Runtime Config Reload

The running service reloads `~/.egrex/config.toml` automatically.

Changes to these fields are applied without restarting:

- `host`
- `port`
- `check_url`
- `check_fallback_url`
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
fofa_state.toml
```

Do not commit `~/.egrex/config.toml`; it may contain private FOFA credentials.
