# Egrex

Egrex is a local SOCKS5 proxy forwarder with an upstream SOCKS5 proxy pool. It listens locally and rotates outbound requests through verified upstream SOCKS5 nodes.

## 中文说明

Egrex 是一个本地 SOCKS5 代理池转发工具。它会在本地监听一个 SOCKS5 代理地址，并从在线上游代理池中轮转选择 SOCKS5 节点进行转发。

后台运行后，Egrex 会自动维护两个地址池：

```text
candidates.lock  候选地址池，目标数量 200
online.lock      在线地址池，目标数量 80
```

主要流程：

```text
FOFA API
  -> 候选地址池
  -> 延时和可用性检测
  -> 在线地址池
  -> 本地 SOCKS5 转发
```

### 中文快速开始

设置本地监听地址：

```bash
cargo run -- set host 127.0.0.1
cargo run -- set port 1080
```

设置 FOFA：

```bash
cargo run -- set fofa-api https://fofa.info/api/v1/
cargo run -- set fofa-key <your_api_key>
```

设置检测地址和最大允许延时，延时单位是毫秒：

```bash
cargo run -- set check-url https://cloudflare.com/cdn-cgi/trace
cargo run -- set max-latency 5000
```

启动后台代理：

```bash
cargo run -- start
```

然后把浏览器、系统代理、curl 或其他应用的代理设置为：

```text
SOCKS5 127.0.0.1:1080
```

查看运行状态、监听地址、流量和地址池状态：

```bash
cargo run -- status
```

温和停止后台代理，已有连接会尽量自然结束：

```bash
cargo run -- stop
```

强制停止：

```bash
cargo run -- stop --force
```

### 中文命令参考

```bash
cargo run -- run                 # 前台运行
cargo run -- start               # 后台运行
cargo run -- stop                # 温和停止
cargo run -- stop --force        # 强制停止
cargo run -- status              # 查看状态
cargo run -- update              # 从 FOFA 拉取候选代理
```

配置命令：

```bash
cargo run -- set host <host>
cargo run -- set port <port>
cargo run -- set check-url <url>
cargo run -- set max-latency <milliseconds>
cargo run -- set fofa-api <url>
cargo run -- set fofa-key <key>
```

多单词配置项也支持下划线别名，例如 `fofa_key`、`fofa_api`、`check_url`、`max_latency`。

运行中的服务会自动重新加载 `config.toml`。修改 `host` 或 `port` 后，监听器会重新绑定到新地址；已有连接会继续处理到结束。

`config.toml`、`pid.lock`、`traffic.lock`、`candidates.lock`、`online.lock` 等运行时文件不会提交到 Git。`config.toml` 可能包含 FOFA key，请不要手动提交。

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
