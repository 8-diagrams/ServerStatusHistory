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

事件枚举包括主机状态、资源告警和资源恢复：

| 事件 | 含义 | 典型用途 |
| --- | --- | --- |
| `NodeUp` | 主机在一段时间没有上报后重新上报 | 恢复通知 |
| `NodeDown` | 服务端判定主机从在线变为离线 | 掉线通知 |
| `Custom` | 主机仍在线，周期性执行自定义规则 | 内存、磁盘等指标阈值 |
| `AlertRecovered` | 资源告警从 active 降到 `recover_threshold` 或以下 | 磁盘、内存等恢复通知 |

这不是一个带告警等级、确认（ack）、静默窗口和升级策略的完整规则引擎。当前版本已为磁盘和内存占用告警增加 SQLite 收敛状态；其他 `Custom` 规则仍由模板或 Webhook 脚本自行决定是否输出内容。

## 2. 事件触发逻辑

### 2.1 上线和掉线

- `offline_threshold` 是无上报多久后判定离线，默认 `30` 秒，服务端会把小于 `30` 的值强制改为 `30`。
- 每次成功进入服务端统计队列后，服务端用自己的接收时间写入 `Host.latest_ts`；因此离线判断依赖“服务端最后收到数据的时间”，不依赖 Agent 携带的事件时间。
- 定时线程每 500ms 检查：当 `now > latest_ts + offline_threshold` 时，将 IPv4/IPv6 在线状态置为离线。机器只有从在线状态变为离线时才产生一次 `offline candidate` 日志，日志包含 `last_received_at`、`gap_seconds`、`offline_threshold`、`online4` 和 `online6`；实际生成 `NodeDown` 时还会输出 `node down` 日志。
- 主机从在线状态变为离线时产生一次 `NodeDown`。
- 只有服务端先确认该主机进入离线状态并产生 `NodeDown` 后，恢复上报才会产生一次 `NodeUp`；单纯一次上报延迟超过 `offline_threshold` 不再伪造上线恢复事件。
- Agent 进程重启本身不是 `NodeUp` 事件；如果重启期间的上报间隔没有超过 `offline_threshold`，服务端会继续按在线主机处理，只会产生普通的 `Custom` 检查事件。
- 发送 `NodeDown` 后，运行时主机状态会标记为已确认离线，避免每个定时周期重复发送；下一次有效上报会生成 `NodeUp` 并清除离线状态。
- 事件使用服务端记录的最后上报时间进行状态判断；Agent 的事件时间主要用于历史数据，不能用来绕过服务端的离线判断。
- 如果上报 JSON/Protobuf 反序列化失败，HTTP `/report` 会记录 `report error` 并返回 `400`；有效旧 Agent 的请求格式不变。
- 固定主机启动时会预先出现在列表中，即使离线也会保留；尚未成功上报的固定主机不会伪造一次 `NodeDown`。动态 `hosts_group` 主机则受 `group_gc` 清理策略影响。

### 2.2 自定义规则

服务端定时对在线且允许通知的主机产生 `Custom` 事件。通知器自己决定是否真正发送：

- Telegram、企业微信、邮件：渲染 `custom_tpl`，内容为空则不发送。
- 文件日志：使用独立的 `log.tpl`，每次事件都写日志；是否有内容由模板决定。
- Webhook：Rhai 脚本必须返回 `[是否发送, 请求体]`，第一个元素为 `false` 时不发 HTTP 请求。

因此，若自定义模板条件持续满足，告警会按通知间隔重复发送；当前没有“只在首次越过阈值时发送一次”的内置边沿检测。

### 2.3 资源告警恢复

资源告警恢复是独立事件，不复用 `NodeUp`。例如磁盘从 85% 降到 75% 或以下时：

1. 服务端删除该主机、规则和告警类型对应的 SQLite 状态；
2. 记录 `alert resolved` 日志；
3. 生成一次 `AlertRecovered` 事件；
4. 使用各通知器的 `recovery_tpl` 发送恢复通知；
5. 在下一次仍低于恢复阈值的检查中不重复发送。

如果磁盘只从 85% 降到 78%，仍处于恢复滞回区间，不会恢复，也不会重复发送恢复通知。之后再次超过 80% 会开始新的告警周期。

默认恢复模板可以使用：

```jinja
{% for recovery in recoveries %}
✅ {{host.name}} {{recovery.alert_type}} 已恢复，
当前 {{recovery.value_percent | round}}%，
恢复阈值 {{recovery.recover_threshold}}%
{% endfor %}
```

当前内置的 Telegram、企业微信和邮件通知器支持 `recovery_tpl`；文件日志通过 `recoveries` 字段记录恢复事件；Webhook Rhai 脚本可以使用 `event == "AlertRecovered"` 和 `recoveries` 数组自行构造消息。旧配置缺少 `recovery_tpl` 时使用内置默认模板，不影响 Agent 或旧配置启动。

### 2.4 通知节流和允许开关

全局配置：

```toml
offline_threshold = 30 # 最小 30 秒
notify_interval = 30   # 最小 30 秒
```

`notify_interval` 是服务端事件检查的节流间隔，按主机分别记录。某一台主机的频繁 `Custom` 检查不会再阻塞另一台主机的 `NodeDown` 判定。

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

## 3.1 事件优先的渠道路由与多格式渲染

通知采用“先事件、后渠道”的模型：服务端先生成一个标准事件，再依据事件配置选择渠道；每个渠道再选择最适合自己的表示格式。这样同一个磁盘告警可以同时渲染 HTML、纯文本和结构化 JSON，而不必为每个渠道重复写一套告警判断。

事件路由写在 `[alerts.events.<event>]` 下：

```toml
[alerts.events.disk_usage_high]
channels = ["all"]                 # 所有已启用渠道

[alerts.events.disk_usage_recovered]
channels = ["wechat", "tgbot"]    # 只投递给指定渠道

[alerts.events.node_down]
channels = []                       # 记录/维护状态，但不发送
```

规则说明：

- `channels = ["all"]` 选择所有已启用的通知器；
- `channels = ["wechat", "tgbot"]` 只选择列出的通知器；
- `channels = []` 明确关闭该事件的投递，但事件检测、状态和日志仍然保留；
- `exclude_channels = ["email"]` 可以在 `all` 的基础上排除渠道；
- 未配置的事件保持旧版兼容行为：广播到所有已启用渠道；
- 事件名不区分大小写，`NodeDown`、`node_down` 会归一化为同一个路由名。

事件模板位于同一个事件下：

```toml
[alerts.events.disk_usage_high.templates]
title = "主机 {{ host.name }} 磁盘告警"
html = "<b>{{ host.name }}</b> 磁盘使用率 {{ firing[0].value_percent | round }}%"
text = "{{ host.name }} 磁盘使用率 {{ firing[0].value_percent | round }}%"
structured = """{"event_type":"disk_usage_high","host":"{{ host.name }}","value":{{ firing[0].value_percent }}}"""
```

例如上线事件可以把标题和正文分开配置：

```toml
[alerts.events.node_up.templates]
title = "主机 {{ host.name }} 恢复上线"
html = "<b>主机恢复上线</b><br/>主机：{{ host.name }}<br/>位置：{{ host.location }}"
text = "主机恢复上线\n主机：{{ host.name }}\n位置：{{ host.location }}"
```

渠道的 `format` 决定选择哪个版本。默认值如下：

| 渠道 | 默认格式 | 说明 |
| --- | --- | --- |
| `tgbot` | `html` | Telegram HTML parse mode |
| `wechat` | `text` | 企业微信文本消息 |
| `email` | `html` | HTML 邮件正文 |
| `log` | `text` | 文件日志 |
| `webhook` | `structured` | 发送统一的 `title/body` JSON |

可以在渠道段覆盖：`format = "html"`、`format = "text"` 或 `format = "structured"`；`json`/`webhook` 是 `structured` 的别名，`txt`/`plain` 是 `text` 的别名。事件模板优先于旧的 `online_tpl`、`offline_tpl`、`custom_tpl` 和 `recovery_tpl`；当目标事件没有对应格式模板时，自动回退到旧渠道模板，因此旧配置无需修改。

通知器内部使用统一的消息对象：

```text
message.title = alerts.events.<event>.templates.title 渲染结果
message.body  = alerts.events.<event>.templates.<html|text|structured> 的渲染结果
```

通知器不需要知道事件是 `node_up`、`node_down` 还是磁盘告警，只负责把 `title` 和 `body` 映射到自己的协议。Telegram、企业微信通常发送 `title + body`，邮件将二者组成正文，日志写入同一文本；Webhook 在事件级 `structured` 模式下发送统一 JSON：`{"title":"...","body":<结构化内容>}`。事件没有配置 `templates.title` 时，`title` 回退到原有通知渠道配置；没有事件级 body 模板时，`body` 回退到旧的 `online_tpl`、`offline_tpl`、`custom_tpl` 或 `recovery_tpl`。

Webhook 仍然完全兼容原有 `receiver.script`。当 `[alerts.events.<event>.templates].structured` 存在时，所有启用的 receiver 收到同一份 `title/body` JSON（各 receiver 的 URL、headers、认证仍独立）；没有 structured 模板时才执行原有 Rhai 脚本并按 `[true, body]` 决定是否发送。这样可以逐事件迁移，不会破坏已有 Discord、Slack、飞书等脚本。

路由与渲染日志示例：

```text
alert event detected event=AlertRecovered host=node-1
alert delivery skipped notifier=tgbot reason=route_not_selected
alert render selected notifier=webhook event=AlertRecovered host=node-1 format=structured
alert delivery accepted notifier=wechat event=AlertRecovered host=node-1 status=queued
alert delivery sent notifier=webhook event=AlertRecovered host=node-1 status=200 OK
```

完整的通知审计链路还会记录以下阶段（默认 `alerts.log_delivery = true`）：

```text
alert event detected event=NodeDown host=node-1 gid=MAILSVR route_keys=[node_down]
alert delivery queue=accepted event=NodeDown host=node-1
alert channel selected notifier=tgbot event=NodeDown host=node-1 route_keys=["node_down"]
alert processing notifier=tgbot event=NodeDown host=node-1 firing=0 recoveries=0 suppressed_rules=0
alert content assembled notifier=tgbot event=NodeDown format=html route_keys=["node_down"] title="主机 node-1 离线告警" body_len=...
alert channel request assembled notifier=tgbot event=NodeDown host=node-1 endpoint=sendMessage chat_id_configured=true parse_mode=HTML body_len=...
alert delivery sent notifier=tgbot event=NodeDown host=node-1 status=200 OK
```

日志中的 `body_preview` 只记录经过换行压缩并限制长度的正文摘要；Telegram chat id、Bot token、企业微信 secret、SMTP 密码、Webhook Basic Auth 等敏感值不会记录。若需要排查完整模板内容，应直接检查配置文件并保护好其权限。

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
recovery_tpl = "{% for recovery in recoveries %}✅ {{host.name}} {{recovery.alert_type}} 已恢复{% endfor %}"
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
| `recovery_tpl` | `AlertRecovered` 的 Jinja 模板；上下文包含 `recoveries` |
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
recovery_tpl = "{% for recovery in recoveries %}✅ {{host.name}} {{recovery.alert_type}} 已恢复{% endfor %}"
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
recovery_tpl = "{% for recovery in recoveries %}✅ {{host.name}} {{recovery.alert_type}} 已恢复{% endfor %}"
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
| `[webhook].title` | 事件模板未配置 `title` 时使用的默认标题 |
| `receiver[].enabled` | 单个目标开关 |
| `url` | POST 目标 |
| `headers` | 任意 HTTP 请求头，通常至少设置 `content-type` |
| `username` / `password` | 可选 HTTP Basic Auth；两者都非空才启用 |
| `timeout` | HTTP 超时秒数，示例为 5 |
| `script` | Rhai 脚本，负责按事件构造请求体 |

脚本运行时可用变量：

- `event`：字符串 `NodeUp`、`NodeDown`、`Custom` 或 `AlertRecovered`
- `host`：主机对象
- `config`：当前 receiver 配置对象
- `ip_info`：主机 IP 信息（可能为空）
- `sys_info`：主机系统信息（可能为空）

事件级 `structured` 模板还可以直接使用 `firing` 和 `recoveries` 数组；其中每个 firing 包含 `alert_type`、`rule_id`、`value_percent`、`threshold`、`recover_threshold`，每个 recovery 包含 `alert_type`、`rule_id`、`value_percent` 和 `recover_threshold`。

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
event      事件名：NodeUp、NodeDown、Custom 或 AlertRecovered
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

## 6. 旧版 `config.toml` 的兼容与默认值来源

告警功能升级必须兼容现有线上配置。旧配置通常没有 `[alerts]`、`[[alerts.rules]]` 或收敛策略段；这类配置不能因为缺少新字段而启动失败。新版加载流程应遵循以下优先级：

```text
显式配置值 > 合法化后的显式值 > 系统预设值
```

旧配置缺少新字段时自动采用系统预设值；显式配置但超出允许范围时，使用修正后的安全值，并记录 `adjusted` 日志。Agent 的 HTTP/gRPC 上报协议不变，Agent 无需升级。

### 6.1 告警默认预设

下面的预设值用于没有对应配置项的旧版 `config.toml`。它们应集中定义在 Rust 配置模块中，不能在通知器或 `StatsMgr` 内分别写一套默认值。

| 配置路径 | 预设值 | 作用 |
| --- | --- | --- |
| `alerts.enabled` | `true` | 保留旧版告警能力；主机和通知器自身的 `notify/enabled` 仍然有效 |
| `alerts.state_path` | `data/alerts.db` | 保存告警指纹、发送次数和收敛状态；相对路径按服务端工作目录解析 |
| `alerts.log_suppressed` | `true` | 记录被收敛的告警，但采用“首次抑制 + 下次发送汇总”策略，避免每 30 秒刷屏 |
| `alerts.log_delivery` | `true` | 记录发送成功、发送失败和队列投递失败 |
| `alerts.offline_log` | `true` | 记录离线候选、最后接收时间、间隔秒数和离线阈值 |
| `alerts.convergence_rules[0].windows[0]` | `max_notifications = 3`, `interval = "5m"` | 第 1～3 次实际发送的告警，5 分钟内最多发送一次 |
| `alerts.convergence_rules[0].windows[1]` | `max_notifications = 3`, `interval = "10m"` | 第 4～6 次实际发送的告警，10 分钟内最多发送一次 |
| `alerts.convergence_rules[0].windows[2]` | `interval = "60m"` | 第 7 次及以后，60 分钟内最多发送一次 |
| `alerts.disk_usage.recover_threshold` | `75%` | 默认磁盘告警阈值为 80% 时，降至 75% 才恢复，避免临界值抖动 |
| `alerts.memory_usage.threshold` / `recover_threshold` | `80%` / `75%` | 内存告警的默认触发和恢复阈值；与磁盘共享默认收敛规则，但状态独立 |

收敛次数按“实际发送次数”计数，不按后台每轮检查次数计数。告警恢复后清零；服务端重启后从 `state_path` 恢复。`notify_interval` 仍然是事件检查周期，不能被解释为上述收敛时间。

收敛规则独立于告警类型定义。一个规则可以通过 `alert_types` 绑定多个告警项目，`*` 表示匹配未被更具体规则覆盖的项目：

```toml
[[alerts.convergence_rules]]
id = "resource-default"
alert_types = ["disk_usage_high", "memory_usage_high"]

[[alerts.convergence_rules.windows]]
max_notifications = 3
interval = "5m"

[[alerts.convergence_rules.windows]]
max_notifications = 3
interval = "10m"

[[alerts.convergence_rules.windows]]
interval = "60m"
```

同一个告警类型优先使用第一个精确匹配规则；没有精确匹配时才使用 `*` 规则。告警状态指纹包含规则 ID、告警类型、主机和目录，因此同一规则下的不同主机或不同告警类型不会互相计数。

### 6.2 `custom_tpl` 的精确匹配

Jinja 的自由文本输出本身没有告警 ID，服务端不能可靠地从“硬盘使用率超过 80%”这类自然语言中反推它属于哪条规则。因此新版采用显式标记块，标记中的 ID 必须与 `alerts.convergence_rules[].alert_types` 中的值一致：

```jinja
{% if host.hdd_total > 0 and host.hdd_used / host.hdd_total > 0.8 %}
<!-- ss-alert:disk_usage_high -->
<pre>{{ host.name }} 硬盘使用率超过 80%</pre>
<!-- /ss-alert -->
{% endif %}

{% if host.memory_total > 0 and host.memory_used / host.memory_total > 0.8 %}
<!-- ss-alert:memory_usage_high -->
<pre>{{ host.name }} 内存使用率超过 80%</pre>
<!-- /ss-alert -->
{% endif %}
```

服务端先按指标计算收敛状态，再把被收敛的规则 ID 传给通知器。通知器渲染模板后只移除对应标记块，其他告警块仍可发送。例如磁盘告警被抑制而内存告警未被抑制时，只移除 `disk_usage_high` 块，不会丢弃内存消息。

模板条件建议同时检查对应的 `enabled`：`alerts.disk_usage.enabled` 或 `alerts.memory_usage.enabled`。这样关闭某一类结构化告警时，后端检测、收敛和模板输出会保持一致；阈值条件则统一引用 `alerts.*.threshold`，不要在模板中再次硬编码百分比。

旧的未标记 `custom_tpl` 仍可以启动和发送，启动时会输出 `custom template is unclassified` 警告；当需要抑制其中某一条告警时，因为无法安全拆分，服务端会保守地抑制整段旧模板。要获得精确收敛，必须给每个分支加上上述标记。服务端不会按文案、正则或模板渲染结果猜测规则 ID，避免误匹配。

目前标记解析已接入 Telegram、企业微信和邮件的 `custom_tpl`。Webhook 的传统 Rhai 脚本仍可自行决定发送内容；若改用事件级 `structured` 模板，则磁盘/内存告警和恢复事件会直接使用服务端结构化上下文。

旧模板未加标记时，资源分支仍使用对应告警的状态指纹，但无法做到同一模板内部的独立收敛；迁移到标记块后，状态指纹使用 `rule_id:<alert_type>:<host>:<gid>`，使不同告警互不影响。

### 6.3 必须输出的配置来源日志

服务端启动时应输出一条不含敏感值的配置摘要，并对每一个新增告警配置标记来源：

```text
INFO config preset selected section=alerts key=enabled value=true source=default reason=missing
INFO config preset selected section=alerts key=convergence_rules[0].windows[0].interval value=5m source=default reason=missing
INFO config preset selected section=alerts key=state_path value=data/alerts.db source=default reason=missing
INFO config effective section=alerts enabled=true state_path=data/alerts.db convergence_rules=1
```

建议的来源枚举为：

- `explicit`：配置文件中明确写出且通过校验；
- `default`：字段缺失，采用系统预设；
- `adjusted`：字段存在但被限制到安全范围，例如小于最小间隔；
- `generated`：运行时生成，例如空的 `jwt_secret` 或 `admin_pass`。

日志中不得输出密码、JWT secret、Bot token、SMTP password、Webhook basic-auth password 等原文，只输出 `configured`、`generated` 或 `redacted`。当前 `config::from_str` 会打印 `admin_pass`，新版应移除该行为并改为：

```text
WARN config generated key=admin_pass source=generated value=redacted
WARN config generated key=jwt_secret source=generated value=redacted
```

配置加载失败仍应直接失败并给出字段路径；“字段缺失”不是错误。未知字段为了兼容旧版本不应阻止启动，但应输出：

```text
WARN config unknown_key path=alerts.old_option action=ignored
```

`config inspect` 工具则应把这些信息转换为报告，展示原始值（敏感字段脱敏）、有效值、来源和警告，而不是只做 TOML 语法检查。

### 6.4 加载实现建议

不要让 `#[serde(default)]` 成为唯一的可观察来源。建议保留现有 `Config` 结构用于业务代码，同时增加一个统一加载入口：

```rust
pub struct ConfigLoadReport {
    pub config: Config,
    pub fields: Vec<ConfigFieldState>,
    pub warnings: Vec<ConfigWarning>,
}
```

加载入口先将 TOML 解析为 `toml::Value` 记录字段是否出现，再反序列化为 `Config`，统一执行默认值、范围修正、路径解析和敏感值脱敏。现有 `from_file`、`from_env` 和 `--cloud` 都调用同一个入口；`from_str` 可保留为兼容包装器。这样线上旧配置、环境变量配置和审计工具会得到完全一致的最终生效结果。

配置报告建议支持：

```bash
./stat_server config inspect -c /opt/ServerStatus/config.toml
./stat_server config inspect -c /opt/ServerStatus/config.toml --format json --redact
./stat_server config diff -c /opt/ServerStatus/config.toml --reference ./config.toml
```

退出码建议为 `0=有效`、`1=存在错误`、`2=有效但有警告`。审计命令必须使用与服务启动相同的加载逻辑，不能另写一套默认值。

### 6.5 升级与回滚行为

1. 直接替换新版二进制并使用旧 `config.toml`，服务端应正常启动。
2. 启动日志明确显示 `[alerts]` 缺失，因此选择了哪些预设；不要求用户立即修改配置。
3. 用户只需要在需要覆盖时增加 `[alerts]` 或对应规则；未覆盖的字段继续使用预设。
4. 回滚到旧二进制时，新版新增的 TOML 段可能被旧版忽略，因此部署文档应提示：若需要回滚，先保留一份旧格式配置，避免把新版专用段带回旧程序。
5. 告警状态库是可选的：若路径不可写，应记录明确错误。第一版可降级为内存状态并给出 `WARN`，但必须说明重启会丢失收敛计数；生产环境建议将 `data/alerts.db` 放在可持久化目录并纳入备份。

### 6.6 使用 `config migrate` 升级旧配置

新版服务端内置配置迁移命令。它只改造告警相关配置，不改变 Agent 上报协议，也不会删除旧配置注释或其他业务段落：

```bash
# 先预览，不写文件、不创建备份
./stat_server --config /opt/ServerStatus/config.toml config migrate --dry-run

# 原地升级；成功前先创建备份
./stat_server --config /opt/ServerStatus/config.toml config migrate

# 写入新文件，不覆盖已有目标文件
./stat_server --config /opt/ServerStatus/old.toml config migrate \
  --output /opt/ServerStatus/config.new.toml

# 目标文件已存在时，明确允许覆盖
./stat_server --config /opt/ServerStatus/old.toml config migrate \
  --output /opt/ServerStatus/config.new.toml --force
```

迁移前会先校验输入 TOML，生成结果后再次校验；实际写入使用临时文件再原子替换。原地迁移会在同目录创建：

```text
config.toml.bak.20261107010901
```

如果同一秒已有同名备份，会自动追加 `.1`、`.2` 等后缀。`--dry-run` 不会创建任何备份。即使配置已经是新版格式，命令仍会保留一份原文件备份，并报告 `changes=0`，便于审计。

迁移器会补齐缺失的 `[alerts]`、`disk_usage`、`memory_usage`、收敛规则和模板标记；已有字段的值不会被默认值覆盖。旧模板中无法识别的自定义写法不会强行改写，命令完成后应人工检查模板和日志。

迁移完成后建议执行：

```bash
./stat_server --config /opt/ServerStatus/config.toml -t
```

确认无误后再重启服务端。回滚时先停止服务，再用备份替换配置，例如：

```bash
cp /opt/ServerStatus/config.toml.bak.20261107010901 \
   /opt/ServerStatus/config.toml
```

若要回滚到旧版二进制，建议同时恢复迁移前的配置；旧版可能忽略或不理解新版专用的 `[alerts]` 段。备份文件中可能包含通知凭据，应与原配置使用同样的文件权限保护。

## 7. 推荐设置方案

### 仅监控上下线

打开任意一种通知器，保留 `online_tpl` 和 `offline_tpl`，将 `custom_tpl` 设为空。适合先验证链路。

### 监控资源阈值

使用 `custom_tpl` 或 Webhook Rhai：

- 内存：`memory_used / memory_total`
- 硬盘：`hdd_used / hdd_total`
- CPU：`cpu / 100`
- Swap：`swap_used / swap_total`

磁盘告警可直接配置：

```toml
[alerts.disk_usage]
enabled = true
threshold = 80.0
recover_threshold = 75.0
```

磁盘或内存占用达到各自的 `threshold` 后，服务端在产生 `Custom` 事件前按 `alerts.convergence_rules` 收敛；降到各自的 `recover_threshold` 或更低时清除状态。其他尚未结构化的自由格式 `Custom` 告警仍受 `notify_interval` 影响，建议为这类规则配置合适的检查周期。

### 只记录不推送

打开 `[log]`，关闭其他适配器。之后用日志系统做检索、聚合或二次通知。

### 同时推送多个渠道

同时将多个顶层适配器设为 `enabled = true`；同一事件会广播到所有已启用适配器。Webhook 可以配置多个 receiver，每个 receiver 可有不同脚本和目标。

## 8. 排查清单

1. 先执行 `./stat_server -c config.toml -t`，确认 TOML 语法。
2. 再执行 `./stat_server -c config.toml --notify-test`，确认凭据、网络和目标地址。
3. 查看服务端日志：建议使用 `RUST_LOG=info` 或临时使用 `RUST_LOG=trace`。
4. 确认主机和 Agent 没有设置 `notify = false` 或 `--disable-notify`。
5. 确认服务端实际判定的 `offline_threshold` 和 `notify_interval` 均不小于 30 秒。
6. 自定义规则没有消息时，先把模板改成固定文本，判断是“规则条件不满足”还是“传输失败”。
7. Webhook 若启动即失败，优先检查 Rhai 脚本语法；若启动成功但无消息，检查返回值是否确实为 `[true, body]`。
8. 邮件检查 SMTP 是否允许 STARTTLS，以及 `to` 中的地址格式。
9. 日志通知检查 `log_dir` 权限，并用 logrotate 管理文件大小和保留时间。

## 8.1 用服务端日志追踪告警和通知

建议生产服务使用 `RUST_LOG=info`。一次恢复事件的典型链路如下：

```text
alert resolved ... alert_type=disk_usage_high ...
alert event detected event=AlertRecovered host=node-1 gid=...
alert delivery queue=accepted event=AlertRecovered host=node-1
alert delivery accepted notifier=tgbot event=AlertRecovered host=node-1 status=queued
alert delivery sent notifier=tgbot event=AlertRecovered host=node-1 status=200 OK
```

各日志含义：

- `alert resolved`：告警状态机确认发生了 active → recovered，并清理 SQLite 状态；
- `alert event detected`：服务端生成了独立事件；
- `alert delivery queue=accepted`：事件成功进入通知队列；如果出现 `queue_failed`，表示还没有到达任何通知器；
- `alert delivery accepted`：某个通道的模板已经有内容，并接受了发送任务；`skipped` 表示模板为空、Webhook 脚本过滤或没有启用 receiver；
- `alert delivery sent`：异步 HTTP、SMTP 或文件写入完成；
- `alert delivery failed`：通道连接、认证、SMTP、Webhook 或文件写入失败。

例如只看恢复事件：

```bash
RUST_LOG=info ./stat_server --config /opt/ServerStatus/config.toml 2>&1 \
  | grep -E 'alert (resolved|event detected|delivery)|AlertRecovered'
```

如果服务由 systemd 管理：

```bash
journalctl -u serverstatus -f -o cat \
  | grep -E 'alert (resolved|event detected|delivery)|AlertRecovered'
```

只检查某台主机：

```bash
journalctl -u serverstatus -f -o cat \
  | grep -E 'host=node-1|host="node-1"'
```

注意：`accepted/queued` 表示服务端已把任务交给异步通道，不等同于对端业务确认；是否真正完成，要继续观察同一 `notifier` 的 `sent` 或 `failed` 日志。Telegram、企业微信、Webhook 会记录 HTTP 状态，邮件记录 SMTP 成功或失败，文件日志记录写入文件路径。

使用 systemd 部署时，服务端标准输出和错误输出默认进入 journald。推荐使用：

```bash
# 持续查看服务端日志
journalctl -u stat_server -f -o cat

# 查看最近一小时的事件和通知链路
journalctl -u stat_server --since "1 hour ago" -o cat \\
  | grep -E 'offline candidate|node down|alert event detected|alert processing|alert channel selected|alert content assembled|alert channel request assembled|alert delivery'

# 只看某台主机
journalctl -u stat_server -f -o cat \\
  | grep -E 'host=node-1|host="node-1"'
```

仓库中的 [systemd/stat_server.service](systemd/stat_server.service) 已设置 `RUST_LOG=info`、`StandardOutput=journal` 和 `StandardError=journal`。安装或修改 unit 文件后执行：

```bash
sudo systemctl daemon-reload
sudo systemctl restart stat_server
sudo systemctl status stat_server
```

如果只想临时提高日志级别，可以停止 systemd 服务后前台运行：

```bash
RUST_LOG=debug /opt/ServerStatus/stat_server \\
  --config /opt/ServerStatus/config.toml
```

`[log]` 通知器是另一类输出：它把事件模板内容写到 `log_dir/ssr.log.YYYY-MM-DD`，不能替代 journald 中的服务端处理链路日志。建议同时保留 journald，便于判断事件是否产生、路由是否命中以及通道发送是否成功。

## 9. 当前实现边界和后续可改进点

- `notify_interval` 当前按主机节流；磁盘和内存占用告警另外使用各自的持久化收敛规则。尚未结构化的 `Custom` 告警仍没有独立的规则级节流。
- 磁盘和内存占用告警已经按规则、主机和目录持久化收敛状态；其他尚未结构化的 `Custom` 告警仍没有独立的阈值边沿状态。
- 未标记的自由格式 `custom_tpl` 无法把最终渲染内容反向映射为具体告警类型，因此在资源告警收敛时会保守地整体抑制；使用 `ss-alert` 标记块后，服务端只移除对应告警块，其他告警可以继续发送。
- `AlertRecovered` 目前只覆盖结构化的磁盘和内存规则；尚未结构化的自由格式 `Custom` 规则不会自动推导恢复事件。
- Webhook 是同步构建脚本、异步发送 HTTP；没有重试、签名、失败队列或持久化投递记录。
- 企业微信每次发送都重新取 token，没有 token 缓存。
- 邮件适配器固定使用 STARTTLS；没有连接池、重试和可配置 TLS 模式。
- 发送失败主要记录日志，不会反向影响主机状态，也不会自动切换到其他通知渠道。
- 配置文件包含 token、密码和 SMTP 凭据，应限制文件权限，避免提交到公共仓库。
- `[email]` 启用后必须提供可解析的 `to` 地址；当前实现对无效地址使用 `expect`，配置错误可能导致发送任务 panic。

如果后续要扩展为更完整的告警系统，建议把“规则定义、状态机、抑制/恢复、限流、投递重试”从当前 `Notifier` 适配器中拆出，通知器只负责接收标准化告警事件并投递。
