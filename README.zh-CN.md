# Egrex

[English](README.md) | [简体中文](README.zh-CN.md)

Egrex 是一个本地 SOCKS5 代理池转发工具。它会在本地监听一个 SOCKS5 代理地址，并从在线上游 SOCKS5 节点池中轮转选择节点进行转发。

## 功能

- 本地无认证 SOCKS5 监听器。
- 从在线地址池中轮转选择上游 SOCKS5 节点。
- 基于 FOFA 拉取候选代理。
- 服务运行时自动维护候选地址池和在线地址池。
- 基于延时的上游代理质量过滤。
- 默认温和停止，也支持显式强制停止。
- 支持运行时重新加载 host、port、FOFA 设置、检测 URL 和延时阈值。

## 构建

```bash
cargo build --release
```

构建产物位置：

```text
target/release/Egrex.exe
```

也可以把 `Egrex.exe` 复制到 `PATH` 中的目录，然后直接使用 `Egrex.exe` 命令。

## 快速开始

按需设置本地监听地址：

```bash
Egrex.exe set host 127.0.0.1
Egrex.exe set port 1080
```

设置 FOFA 凭据和 API 地址：

```bash
Egrex.exe set fofa-api https://fofa.info/api/v1/
Egrex.exe set fofa-key <your_api_key>
```

设置上游代理检测地址和最大允许延时，延时单位是毫秒：

```bash
Egrex.exe set check-url https://cloudflare.com/cdn-cgi/trace
Egrex.exe set max-latency 5000
```

后台启动代理：

```bash
Egrex.exe start
```

然后把浏览器、系统代理、curl 或其他应用的代理设置为：

```text
SOCKS5 127.0.0.1:1080
```

查看运行状态：

```bash
Egrex.exe status
```

温和停止，已有连接会尽量自然结束：

```bash
Egrex.exe stop
```

只有在温和停止无法完成时再强制停止：

```bash
Egrex.exe stop --force
```

## 命令

```bash
Egrex.exe run
Egrex.exe start
Egrex.exe stop
Egrex.exe stop --force
Egrex.exe status
Egrex.exe update
```

配置命令：

```bash
Egrex.exe set host <host>
Egrex.exe set port <port>
Egrex.exe set check-url <url>
Egrex.exe set max-latency <milliseconds>
Egrex.exe set fofa-api <url>
Egrex.exe set fofa-key <key>
```

多单词配置项也支持下划线别名，例如 `fofa_key`、`fofa_api`、`check_url` 和 `max_latency`。

## 地址池模型

Egrex 使用两个运行时地址池：

```text
candidates.lock  候选地址池，目标数量 200
online.lock      在线地址池，目标数量 80
```

流程：

```text
FOFA API
  -> candidates.lock
  -> 延时和可用性检测
  -> online.lock
  -> 本地 SOCKS5 转发
```

服务运行时：

- 候选地址池低于目标数量时，会自动从 FOFA 补充。
- 在线地址池会定期检查，节点失败后会从候选地址池补充可用节点。
- 每个新的客户端连接会从在线地址池中按轮转方式选择上游代理。
- 转发过程中失败的上游代理会从在线地址池移除。
- 最近检查过的上游节点会短暂缓存检测结果，避免重复检测。

## 运行时配置重载

服务运行时会自动重新加载 `config.toml`。

下面这些字段可以在不重启服务的情况下生效：

- `host`
- `port`
- `check_url`
- `max_latency`
- `fofa_api`
- `fofa_key`

修改 `host` 或 `port` 后，监听器会重新绑定到新地址。已有客户端连接会继续处理到自然结束。

## 运行时文件

下面这些文件会在本地生成，并且已被 Git 忽略：

```text
config.toml
pid.lock
stop.lock
traffic.lock
upstreams.lock
candidates.lock
online.lock
```

不要提交 `config.toml`，它可能包含私有 FOFA 凭据。
