# ServerStatus 告警通知模型与配置说明

本文根据当前仓库代码整理，适用于 Rust 服务端。告警是服务端行为，Agent 只负责上报指标；修改通知配置不会改变 HTTP/gRPC 上报协议，也不要求升级 Agent。

## 1. 总体模型

当前实现由三层组成：

1. **事件检测**：`StatsMgr` 根据主机最后一次上报时间、在线状态、`notify_interval` 和 `offline_threshold` 产生事件。
2. **通知分发**：事件通过有界队列发送给所有已启用的通知器；每个通知器独立渲染和发送。
3. **通知适配器**：内置 Telegram、企业微信、邮件、文件日志和通用 Webhook 五类适配器。

核心代码位置：

- 事件与通知器接口：[server/src/notifier/mod.rs](server/src/notifier/mod.rs)
- 事件检测与分发：[server/src/stats.rs](server/src/stats.rs)
- 配置结构：[server/src/config.rs](server/src/config.rs)
- Telegram：[server/src/notifier/tgbot.rs](server/src/notifier/tgbot.rs)
- 企业微信：[server/src/notifier/wechat.rs](server/src/notifier/wechat.rs)
- 邮件：[server/src/notifier/email.rs](server/src/notifier/email.rs)
- 文件日志：[server/src/notifier/log.rs](server/src/notifier/log.rs)
- Webhook/Rhai：[server/src/notifier/webhook.rs](server/src/notifier/webhook.rs)
- 当前完整示例：[config.toml](config.toml)

事件枚举只有三种：

| 事件 | 含义 | 典型用途 |
| --- | --- | --- |
| `NodeUp` | 主机在一段时间没有上报后重新上报 | 恢复通知 |
| `NodeDown` | 服务端判定主机从在线变为离线 | 掉线通知 |
| `Custom` | 主机仍在线，周期性执行自定义规则 | 内存、磁盘等指标阈值 |

这不是一个带告警状态库的完整规则引擎：当前没有告警等级、确认（ack）、静默窗口、升级策略或持久化告警实例。`Custom` 规则由模板或 Webhook 脚本自行决定是否输出内容。

## 2. 事件触发逻辑

### 2.1 上线和掉线

- `offline_threshold` 是无上报多久后判定离线，默认 `30` 秒，服务端会把小于 `30` 的值强制改为 `30`。
- 主机从在线状态变为离线时产生一次 `NodeDown`。
- 离线后再次收到上报，如果上一次服务端记录与本次上报的时间间隔超过 `offline_threshold`，产生一次 `NodeUp`。
- 发送 `NodeDown` 后，运行时主机状态会标记为已处理，避免每个定时周期重复发送；下一次有效上报会重新恢复运行状态。
- 事件使用服务端记录的最后上报时间进行状态判断；Agent 的事件时间主要用于历史数据，不能用来绕过服务端的离线判断。
- 固定主机即使离线仍保留在主机列表中；动态 `hosts_group` 主机仍受 `group_gc` 清理策略影响。

### 2.2 自定义规则

服务端定时对在线且允许通知的主机产生 `Custom` 事件。通知器自己决定是否真正发送：

- Telegram、企业微信、邮件：渲染 `custom_tpl`，内容为空则不发送。
- 文件日志：使用独立的 `log.tpl`，每次事件都写日志；是否有内容由模板决定。
- Webhook：Rhai 脚本必须返回 `[是否发送, 请求体]`，第一个元素为 `false` 时不发 HTTP 请求。

因此，若自定义模板条件持续满足，告警会按通知间隔重复发送；当前没有“只在首次越过阈值时发送一次”的内置边沿检测。

### 2.3 通知节流和允许开关

全局配置：

```toml
offline_threshold = 30 # 最小 30 秒
notify_interval = 30   # 最小 30 秒
```

`notify_interval` 是服务端事件检查的全局节流间隔，不是每个通知器各自独立的间隔。当前实现中，多个主机共享该节流时间点：同一轮可能只为一个主机生成 `Custom` 事件。因此主机很多时，不能把它理解为“每台机器每 30 秒都一定发送一次”。

主机是否允许通知由以下条件共同决定：

```text
服务端 hosts[].notify 或 hosts_group[].notify
AND
Agent 上报的 notify 字段
AND
主机未 disabled
```

固定主机默认 `notify = true`；Agent 可以使用 `--disable-notify`（Python Agent 也支持同名选项）关闭该节点通知。`notify = false` 适合网络质量差、经常抖动的节点或静默分组。

## 3. 启用和测试流程

1. 编辑服务端 `config.toml`，只打开需要的适配器。
2. 填入对应凭据、URL 和模板。
3. 先做 TOML 配置检查：

   ```bash
   ./stat_server -c config.toml -t
   ```

4. 启动通知测试。该命令会为每个已启用的通知器发送一条 `ServerStatus test msg`，不需要等待真实主机上下线：

   ```bash
   ./stat_server -c config.toml --notify-test
   ```

   `--notify-test` 会初始化通知器并发送测试消息，然后退出；Webhook 测试会向每个启用的 receiver 直接发送固定文本，不执行 receiver 的业务脚本。

5. 确认无误后正常启动：

   ```bash
   ./stat_server -c config.toml
   ```

配置是在进程启动时读取的，修改后需要重启服务端。服务端启动时会编译 Webhook 的 Rhai 脚本；脚本语法错误会导致启动失败，应先在测试环境验证。

通知目前没有 Web 管理页面或运行时配置 API，只能通过服务端配置加载。普通部署使用 `-c` 指定 TOML；云模式使用 `--cloud`，从环境变量 `SRV_CONF` 读取完整配置：

```bash
export SRV_CONF="$(cat config.toml)"
./stat_server --cloud
```

## 4. 通知方式和字段

所有方式都使用 `enabled = true` 才会初始化。未启用的段可以保留，也可以删除。

### 4.1 Telegram Bot

配置段为 `[tgbot]`：

```toml
[tgbot]
enabled = true
bot_token = "123456:replace-me"
chat_id = "-1001234567890"
title = "❗<b>Server Status</b>"
online_tpl = "{{config.title}}\n😆 {{host.name}} 恢复上线"
offline_tpl = "{{config.title}}\n😱 {{host.name}} 已掉线"
custom_tpl = """
{% if host.memory_total > 0 and host.memory_used / host.memory_total > 0.8 %}
😲 {{host.name}} 内存使用率超过 80%
{% endif %}
"""
```

| 字段 | 说明 |
| --- | --- |
| `enabled` | 总开关 |
| `bot_token` | Telegram Bot API token |
| `chat_id` | 目标会话或群组 ID |
| `title` | 模板中的 `config.title`；Telegram 以 HTML parse mode 发送 |
| `online_tpl` / `offline_tpl` | `NodeUp` / `NodeDown` 的 Jinja 模板 |
| `custom_tpl` | `Custom` 的 Jinja 模板，空内容不发送 |

请求使用 Telegram `sendMessage`，超时为 5 秒。模板中使用 HTML 时应遵守 Telegram HTML 格式。

### 4.2 企业微信应用

配置段为 `[wechat]`：

```toml
[wechat]
enabled = true
corp_id = "wwxxxxxxxx"
corp_secret = "replace-me"
agent_id = "1000002"
title = "❗Server Status"
online_tpl = "{{config.title}}\n{{host.name}} 恢复上线"
offline_tpl = "{{config.title}}\n{{host.name}} 已掉线"
custom_tpl = """
{% if host.hdd_total > 0 and host.hdd_used / host.hdd_total > 0.8 %}
{{host.name}} 硬盘使用率超过 80%
{% endif %}
"""
```

服务端先调用企业微信 `gettoken`，再调用消息发送接口；消息类型是文本，接收人为 `@all`。每次发送都会重新获取 token，凭据必须只保存在服务端配置中。

### 4.3 邮件

配置段为 `[email]`：

```toml
[email]
enabled = true
server = "smtp.example.com"
username = "monitor@example.com"
password = "replace-me"
to = "ops@example.com;oncall@example.com"
subject = "ServerStatus Notification"
title = "❗<b>Server Status</b><br/>"
online_tpl = "{{config.title}} {{host.name}} 恢复上线"
offline_tpl = "{{config.title}} {{host.name}} 已掉线"
custom_tpl = """
{% if host.memory_total > 0 and host.memory_used / host.memory_total > 0.8 %}
<p>{{host.name}} 内存使用率超过 80%</p>
{% endif %}
"""
```

| 字段 | 说明 |
| --- | --- |
| `server` | SMTP 服务器主机名 |
| `username` / `password` | SMTP 认证 |
| `to` | 收件人列表，当前示例使用分号分隔多个地址 |
| `subject` | 邮件主题 |
| `title` | 模板中可复用的 HTML 前缀 |
| 三个 `*_tpl` | 分别对应三类事件 |

实现使用 SMTP STARTTLS（`lettre::starttls_relay`），不支持在配置中选择明文 SMTP 或 SMTPS 模式。邮件正文为 `text/html`。

### 4.4 文件日志

配置段为 `[log]`：

```toml
[log]
enabled = true
log_dir = "/opt/ServerStatus/logs"
tpl = """{% set obj = dict(event=event, host=host, ip_info=ip_info, sys_info=sys_info) %}{{ obj | tojson }}"""
```

日志按服务端本地日期写入：

```text
/opt/ServerStatus/logs/ssr.log.2026-11-06
```

`tpl` 是 Jinja 模板，能够访问 `event`、`host`、`ip_info`、`sys_info`、`config`。服务端启动时会创建 `log_dir`；目录不可创建时启动失败。该方式适合审计、调试自定义规则，日志轮转和保留周期需要由 logrotate 或其他系统工具负责。

### 4.5 通用 Webhook

配置段为 `[webhook]`，可配置多个 receiver：

```toml
[webhook]
enabled = true

[[webhook.receiver]]
enabled = true
url = "https://example.com/hooks/serverstatus"
headers = { content-type = "application/json" }
username = ""
password = ""
timeout = 5
script = """
let message = `${event}: ${host.name}`;
[message.len() > 0, #{text: message}]
"""
```

字段说明：

| 字段 | 说明 |
| --- | --- |
| `[webhook].enabled` | Webhook 总开关 |
| `receiver[].enabled` | 单个目标开关 |
| `url` | POST 目标 |
| `headers` | 任意 HTTP 请求头，通常至少设置 `content-type` |
| `username` / `password` | 可选 HTTP Basic Auth；两者都非空才启用 |
| `timeout` | HTTP 超时秒数，示例为 5 |
| `script` | Rhai 脚本，负责按事件构造请求体 |

脚本运行时可用变量：

- `event`：字符串 `NodeUp`、`NodeDown` 或 `Custom`
- `host`：主机对象
- `config`：当前 receiver 配置对象
- `ip_info`：主机 IP 信息（可能为空）
- `sys_info`：主机系统信息（可能为空）

注册的 Rhai 辅助函数：

- `to_json(value)`：转 JSON 字符串
- `join(array, separator)`：连接数组内容
- `now_str()`：返回服务端本地时间字符串

脚本必须返回二元数组：

```rhai
[send, body]
```

`send` 可转换为布尔值；只有为 `true` 且数组长度至少为 2 时才 POST。`body` 可以是对象、数组、字符串等，服务端会将其序列化为 JSON 字符串后作为请求体发送。

仓库 `config.toml` 已提供 Discord、Slack、企业微信机器人和 Bark 的完整示例。它们的主要区别只是 `url`、请求头和脚本返回的 JSON 格式；因此也可以接入飞书、钉钉、Gotify 等兼容 HTTP Webhook 的系统。

## 5. 模板可用数据

Jinja 通知器（Telegram、企业微信、邮件、日志）使用以下上下文：

```text
host       主机状态
config     当前通知器配置
ip_info    上报的 IP 信息，可能为空
sys_info   上报的系统信息，可能为空
event      仅 log.tpl 明确提供，值为 NodeUp/NodeDown/Custom
```

常用 `host` 字段包括：

```text
name, alias, location, type, gid, tags,
online4, online6, uptime,
cpu,
load_1, load_5, load_15,
memory_used, memory_total,
swap_used, swap_total,
hdd_used, hdd_total,
network_rx, network_tx, network_in, network_out,
tcp, udp, process, thread,
time_10010, time_189, time_10086,
ping_10010, ping_189, ping_10086
```

不同适配器对对象的序列化路径略有不同；新增规则前建议以 [server/src/payload.rs](server/src/payload.rs) 和已有 `config.toml` 示例为准。计算比例时务必防止总量为 0，例如：

```jinja
{% if host.memory_total > 0 and host.memory_used / host.memory_total > 0.8 %}
...
{% endif %}
```

## 6. 推荐设置方案

### 仅监控上下线

打开任意一种通知器，保留 `online_tpl` 和 `offline_tpl`，将 `custom_tpl` 设为空。适合先验证链路。

### 监控资源阈值

使用 `custom_tpl` 或 Webhook Rhai：

- 内存：`memory_used / memory_total`
- 硬盘：`hdd_used / hdd_total`
- CPU：`cpu / 100`
- Swap：`swap_used / swap_total`

建议先配置 80% 或 90% 阈值，并保留总量大于 0 的保护条件。由于当前 Custom 告警按间隔重复发送，建议把 `notify_interval` 设为 300 秒或更长，避免持续高负载时刷屏。

### 只记录不推送

打开 `[log]`，关闭其他适配器。之后用日志系统做检索、聚合或二次通知。

### 同时推送多个渠道

同时将多个顶层适配器设为 `enabled = true`；同一事件会广播到所有已启用适配器。Webhook 可以配置多个 receiver，每个 receiver 可有不同脚本和目标。

## 7. 排查清单

1. 先执行 `./stat_server -c config.toml -t`，确认 TOML 语法。
2. 再执行 `./stat_server -c config.toml --notify-test`，确认凭据、网络和目标地址。
3. 查看服务端日志：建议使用 `RUST_LOG=info` 或临时使用 `RUST_LOG=trace`。
4. 确认主机和 Agent 没有设置 `notify = false` 或 `--disable-notify`。
5. 确认服务端实际判定的 `offline_threshold` 和 `notify_interval` 均不小于 30 秒。
6. 自定义规则没有消息时，先把模板改成固定文本，判断是“规则条件不满足”还是“传输失败”。
7. Webhook 若启动即失败，优先检查 Rhai 脚本语法；若启动成功但无消息，检查返回值是否确实为 `[true, body]`。
8. 邮件检查 SMTP 是否允许 STARTTLS，以及 `to` 中的地址格式。
9. 日志通知检查 `log_dir` 权限，并用 logrotate 管理文件大小和保留时间。

## 8. 当前实现边界和后续可改进点

- `notify_interval` 是全局节流，尚未做到按主机、按规则、按渠道独立限流。
- `Custom` 没有告警状态持久化，因此服务端重启后不会知道之前是否已经告警。
- 没有恢复事件对应的自定义规则状态；只有主机上下线恢复（`NodeUp`）。
- Webhook 是同步构建脚本、异步发送 HTTP；没有重试、签名、失败队列或持久化投递记录。
- 企业微信每次发送都重新取 token，没有 token 缓存。
- 邮件适配器固定使用 STARTTLS；没有连接池、重试和可配置 TLS 模式。
- 发送失败主要记录日志，不会反向影响主机状态，也不会自动切换到其他通知渠道。
- 配置文件包含 token、密码和 SMTP 凭据，应限制文件权限，避免提交到公共仓库。
- `[email]` 启用后必须提供可解析的 `to` 地址；当前实现对无效地址使用 `expect`，配置错误可能导致发送任务 panic。

如果后续要扩展为更完整的告警系统，建议把“规则定义、状态机、抑制/恢复、限流、投递重试”从当前 `Notifier` 适配器中拆出，通知器只负责接收标准化告警事件并投递。
