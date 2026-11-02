# ServerStatus-Rust 初始版本改造交接说明

> 更新时间：2026-11-02  
> 本文记录当前工作区相对初始版本的主要改造、设计取舍、验证情况、部署方法和后续优化方向，供后续接手者继续开发。

## 1. 改造目标与兼容性原则

本次改造解决两个主要需求：

1. 服务端保存服务器指标历史数据，Web 可以查看实时及较长时间范围的时间序列。
2. 服务端支持为一台服务器配置多个业务标签，Web 可以按标签筛选和分组。

兼容性原则：

- 没有修改现有 HTTP `/report` 和 gRPC Agent 上报协议，也没有修改 protobuf。
- 现有 Rust Agent、Python Agent 无需升级即可继续上报。
- 历史数据和标签均由 Server 端处理，Agent 不承担历史存储，也不能覆盖服务端标签。
- 旧 `config.toml` 没有 `[history]` 时历史功能默认关闭，升级 Server 不会自动创建大量历史数据。
- `/json/stats.json` 只增加了可选的 `tags` 字段；旧 Web 客户端可以忽略这个新增字段。

## 2. 服务端历史指标改造

### 2.1 新增历史存储层

核心文件：`server/src/history.rs`。

新增 `HistoryStore` trait，服务端采集和 HTTP 层不直接依赖 SQLite。当前工厂只实现了 SQLite，但统一的 `HistoryPoint`、`HistoryQuery` 和指标定义为后续 VictoriaMetrics、InfluxDB 插件预留了边界。

当前 SQLite 实现包括：

- `rusqlite`，使用 `bundled` SQLite，部署机器不需要额外安装 SQLite 动态库。
- WAL 模式和 `NORMAL` synchronous。
- `schema_meta` schema 版本表。
- `metric_samples` 历史样本表及主机、时间索引。
- 独立历史写入线程。
- 容量 2048 的有界队列；队列满时丢弃历史样本但不阻塞 Agent 实时上报。
- 主机离线状态变化时额外记录一个离线历史点。

`HistoryStore` 目前是同步接口，SQLite 连接由 `Mutex` 保护。第一版实现简单可靠，但高并发长查询可能占用 Axum 工作线程，后续可考虑异步插件接口或 `spawn_blocking`。

### 2.2 指标类型

保存的指标覆盖：

- 在线状态：IPv4、IPv6。
- Gauge/状态指标：CPU、1/5/15 分钟负载、内存、Swap、磁盘、TCP/UDP、进程/线程、网络速率、三网延迟和丢包。
- 累计计数器：`network_in`、`network_out`。
- 派生区间流量：API 查询时把累计计数器转换为 `traffic_in`、`traffic_out`。

流量差值处理了：

- Agent/服务器重启造成的计数器回退，不产生负值。
- 查询窗口起点落在两个样本之间时，会读取窗口之前的最后一个计数器作为基线，避免首个流量桶少算。

### 2.3 压缩和自动清理

`config.toml` 的 `[history]` 支持：

```toml
[history]
enabled = true
backend = "sqlite"
path = "data/history.db"
compression = "latest"
sample_interval = 60
retention_days = 30
cleanup_interval = 3600
delete_batch_size = 1000
vacuum_after_cleanup = false
```

- `latest`：把时间戳落到 `sample_interval` 时间桶，同一主机、分组和时间桶保留最后一条数据。
- `none`：按 Agent 上报时间保存；由于主键使用秒级时间戳，同一秒内仍只保留最后一条。
- `retention_days = 0`：关闭自动删除。
- 自动清理按 `delete_batch_size` 分批删除，减少长事务影响。
- `vacuum_after_cleanup` 会主动回收文件空间，但会带来额外 I/O 和锁等待，默认关闭。

默认相对路径 `data/history.db` 以 Server 工作目录为基准。systemd 默认工作目录为 `/opt/ServerStatus`，因此实际文件为 `/opt/ServerStatus/data/history.db`；Docker 中为 `/data/history.db`。

### 2.4 历史查询 API

新增接口：

```text
GET /api/history?host=h1&from=<unix>&to=<unix>&step=60&metrics=cpu,load_1,traffic_in,traffic_out
```

行为：

- `from`、`to` 使用 Unix 秒。
- `step` 控制响应时间桶。
- `metrics` 是逗号分隔的白名单指标。
- 单次查询最长 31 天，SQL 最多读取 100000 条。
- 累计流量在时间桶内求差值累计；其他指标当前使用桶内最后值。
- 接口目前与公开的 `/json/stats.json` 一样未增加鉴权。

## 3. Web 前端改造

### 3.1 Theme-Light 集成

已把 MIT 许可的 `ServerStatus-Theme-Light` 源码集成到 `frontend/`，并保留上游 `LICENSE`。

构建产物输出到：

- `web/index.html`
- `web/theme-light-assets/`

服务端 `server/src/assets.rs` 使用 `RustEmbed` 在编译期嵌入整个 `web/`。因此必须先构建前端，再编译 `stat_server`，否则新二进制可能继续携带旧页面。

### 3.2 实时优先、历史补拉

前端策略不是持续轮询历史 API，而是：

1. 继续轮询现有 `/json/stats.json`。
2. 页面打开后，以约 5 秒粒度在内存积累实时指标。
3. 每 30 秒把稀疏快照写入 IndexedDB。
4. 用户切换指标或时间范围时，先合并内存和 IndexedDB 数据。
5. 只对本地不存在的时间区间请求 `/api/history`。
6. 服务端补拉的数据写回 IndexedDB，避免刷新后重复查询。
7. 按 `host + metric + timestamp` 合并去重。

这符合“浏览器打开后优先依赖浏览器能力；查看浏览器未曾保存的历史时才访问 Server”的目标。

### 3.3 IndexedDB 限额

实现位于 `frontend/src/history.ts`：

- 保留时间：30 天。
- 硬预算：512 MiB。
- 清理目标：448 MiB。
- 达到预算、站点总存储超过约 768 MiB，或站点配额使用率超过 80% 时，从最旧数据开始清理。
- 单条记录大小采用 JSON 大小加保守固定开销估算。

因此本主题自己的缓存目标明显低于 1 GiB。当前是估算限额，不是浏览器底层物理文件大小的精确计量；后续可补充浏览器专项压力测试。

### 3.4 时间序列和图表

前端支持：

- CPU、1/5/15 分钟负载。
- 内存、Swap、磁盘使用率。
- 网络上传/下载速率。
- 区间上传/下载流量。
- TCP、UDP、进程数、线程数。
- 三网延迟和丢包。

时间范围支持 5 分钟、1 小时、6 小时、24 小时、7 天、30 天。长时间范围渲染前最多降采样到约 900 点。

## 4. 多标签改造

### 4.1 服务端数据模型

`Host` 和 `HostGroup` 增加：

```toml
tags = ["AWS", "MAILSVR", "MYSQL", "FINTECH"]
```

实现文件：

- `server/src/config.rs`
- `server/src/payload.rs`
- `server/src/stats.rs`

行为：

- 标签去除首尾空白。
- 按大小写不敏感去重，保留第一次出现的展示文本。
- `hosts_group` 标签由动态注册主机继承。
- `HostStat.tags` 设置为 `skip_deserializing`，Agent 即使提交 `tags` 也会被忽略，最终以 Server 配置为准。
- 标签通过 `/json/stats.json` 下发给 Web。
- `labels` 继续用于 `os=...;spec=...` 这类键值元数据，未被改变或复用。

示例：

```toml
hosts = [
  {name = "h1", password = "p1", alias = "支付数据库", tags = ["AWS", "MYSQL", "FINTECH"]},
]

hosts_group = [
  {gid = "mail", password = "pp", tags = ["AWS", "MAILSVR"]},
]
```

标签当前由配置文件管理，修改后需要重启 Server；尚未实现后台 CRUD、数据库持久化或配置热加载。

### 4.2 前端行为

Theme-Light 已增加：

- 卡片模式和列表模式的标签徽章。
- 设置面板中的单标签筛选。
- “按标签分组”开关。
- 多标签服务器会同时出现在多个标签组。
- 没有标签的服务器进入“未标记”组。
- 筛选与分组选项保存在浏览器 localStorage。

## 5. 配置和容器改造

- `server/Cargo.toml` 增加 bundled `rusqlite`。
- `docker-compose.yml` 增加 `./data:/data`，持久化 SQLite 数据。
- 示例 `config.toml` 增加历史配置和标签示例。
- 已修复 `[history]` 的 TOML 位置：所有根键（尤其 `hosts`、`hosts_group`、`workspace`、`notify_interval`）必须位于第一个 `[table]` 之前。不要把根配置写在 `[history]` 后面，否则它们会被解析成 history 的子键。
- 增加回归测试，保证仓库的完整 `config.toml` 能同时解析出 history 和 hosts。

## 6. 已完成验证

当前改造已通过：

```text
cargo test -p stat_server
cargo check -p stat_server
cargo clippy -p stat_server --all-targets -- -D warnings
frontend/node_modules/.bin/vue-tsc --noEmit
npm run lint
npm run build
git diff --check
```

服务端共有 5 项相关单元测试通过，包括：

- 旧配置不会隐式开启历史存储。
- 标签规范化、去重和 HostGroup 继承。
- 完整示例 TOML 层级正确。
- SQLite 分批清理。
- 累计计数器回退、查询起点基线和响应分桶。

另外完成了真实链路验证：

- 使用原有 JSON Agent 上报到 Server，SQLite 写入及 `/api/history` 查询成功。
- Agent 故意上报伪造标签时，最终 JSON 仍只包含 Server 配置的标签。
- 使用真实 Chrome 无头渲染生产页面，节点、标签徽章、标签筛选、标签分组控件和指标选项均正常显示。

## 7. Server 部署方法

### 7.1 升级前检查与备份

源码构建可以在不中断生产服务的构建机上完成。为减少停机，请先完成 7.2 的构建并确认产物存在，再进入下面的停机备份和替换流程。

建议先备份：

```bash
sudo systemctl stop stat_server
sudo cp /opt/ServerStatus/stat_server /opt/ServerStatus/stat_server.bak
sudo cp /opt/ServerStatus/config.toml /opt/ServerStatus/config.toml.bak
sudo cp /opt/ServerStatus/stats.json /opt/ServerStatus/stats.json.bak 2>/dev/null || true
sudo cp -a /opt/ServerStatus/data /opt/ServerStatus/data.bak 2>/dev/null || true
```

SQLite 运行时还可能存在 `history.db-wal`、`history.db-shm`，停服务后备份整个 `data/` 最稳妥。

### 7.2 从当前源码构建

要求：Rust 1.94+、Node.js、pnpm。

```bash
cd ServerStatus-Rust/frontend
pnpm install --frozen-lockfile
pnpm build

cd ..
cargo test -p stat_server
cargo build --release -p stat_server
```

顺序不能反：`pnpm build` 先更新 `web/`，随后 Rust 才会把新 Theme-Light 资源嵌入 `stat_server`。

当前改造尚未发布成上游 Release 时，不要直接运行 `scripts/status.sh -up -s`，否则下载到的仍可能是未包含本改造的公开二进制。应部署本工作区构建的二进制，或先建立正式 Release/镜像流水线。

### 7.3 配置

在原有 `config.toml` 中保留已有账号、主机和通知配置，并加入 `[history]`。建议把 `[history]` 放在所有根配置之后、`[tgbot]` 等通知表之前，参考仓库当前 `config.toml`。

为主机配置标签：

```toml
hosts = [
  {name = "h1", password = "p1", alias = "支付数据库", tags = ["AWS", "MYSQL", "FINTECH"]},
]
```

创建历史目录并确保运行用户可写：

```bash
sudo mkdir -p /opt/ServerStatus/data
# 如果 systemd 服务改成了非 root 用户，请相应 chown：
# sudo chown -R serverstatus:serverstatus /opt/ServerStatus/data

./target/release/stat_server -c /opt/ServerStatus/config.toml -t
```

注意：上面的配置测试应使用新二进制执行。

### 7.4 替换 systemd 二进制

```bash
sudo systemctl stop stat_server
sudo install -m 0755 target/release/stat_server /opt/ServerStatus/stat_server
sudo systemctl daemon-reload
sudo systemctl start stat_server
sudo systemctl status stat_server
sudo journalctl -u stat_server -n 100 --no-pager
```

默认 systemd 服务的 `WorkingDirectory=/opt/ServerStatus`，不要随意删除，否则相对 SQLite 路径会改变。

### 7.5 部署后验证

```bash
curl -fsS http://127.0.0.1:8080/json/stats.json
curl -fsS "http://127.0.0.1:8080/api/history?host=h1&from=<起始Unix秒>&to=<结束Unix秒>&step=60&metrics=cpu,traffic_in"
ls -lh /opt/ServerStatus/data/
```

然后打开 Web：

- 检查 Theme-Light 页面和节点是否正常。
- 在设置中开启“指标图表”。
- 检查标签筛选和“按标签分组”。
- 新部署没有旧历史属于正常现象，历史只从 Server 启用后开始积累。

如果前面有 Nginx 自己托管 `/opt/ServerStatus/web`，除了替换二进制，还要同步当前 `web/` 目录并清理浏览器/CDN 缓存；默认内置静态资源模式只需替换新二进制。

### 7.6 Docker 部署

不要直接假设公开的 `idoge/stat_server:latest` 已包含本工作区改造。应先发布自定义镜像，再修改 compose 的 `image`。

当前仓库 `Dockerfile` 的 builder 仍是 Rust 1.76，而项目声明需要 Rust 1.94，接手者构建镜像前应把 builder 更新为 Rust 1.94+。同时要保证镜像编译 Server 前，`web/` 已由 `frontend` 生产构建更新。

示例流程：

```bash
cd ServerStatus-Rust/frontend
pnpm install --frozen-lockfile
pnpm build
cd ..

# 更新 Dockerfile builder 至 Rust 1.94+ 后：
docker build -t your-registry/stat_server:rework .
docker push your-registry/stat_server:rework
```

修改 `docker-compose.yml`：

```yaml
services:
  stat_server:
    image: your-registry/stat_server:rework
    volumes:
      - ./config.toml:/config.toml
      - ./stats.json:/stats.json
      - ./data:/data
```

启动：

```bash
mkdir -p data
touch stats.json
docker compose pull
docker compose up -d
docker compose logs --tail=100 stat_server
```

## 8. Agent 部署方法

### 8.1 升级结论

本次功能不要求升级 Agent。

- HTTP JSON 上报仍使用原 `/report`。
- gRPC protobuf 未改动。
- 历史记录由 Server 收到实时数据后异步保存。
- 标签由 Server 的 `hosts` 或 `hosts_group` 配置，不需要新增 Agent 参数。

因此推荐升级顺序是：先部署新 Server，确认旧 Agent 正常上报；Agent 保持原版本即可。无需所有节点同步停机或滚动升级。

### 8.2 新装 Rust Agent

Server 的 `hosts` 配置：

```toml
hosts = [
  {name = "h1", password = "p1", alias = "node-1", tags = ["AWS", "MYSQL"]},
]
```

Agent 命令不变：

```bash
/opt/ServerStatus/stat_client \
  -a "https://status.example.com/report" \
  -u h1 \
  -p p1
```

动态主机组：

```toml
hosts_group = [
  {gid = "mail", password = "pp", tags = ["AWS", "MAILSVR"]},
]
```

```bash
/opt/ServerStatus/stat_client \
  -a "https://status.example.com/report" \
  -g mail \
  -p pp \
  --alias "$(hostname)"
```

systemd 的 `ExecStart` 仍按仓库 `systemd/stat_client.service` 配置，修改后执行：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now stat_client
sudo systemctl restart stat_client
sudo journalctl -u stat_client -n 100 --no-pager
```

### 8.3 Python Agent

Python Agent 同样无需改造，继续使用原命令：

```bash
python3 stat_client.py \
  -a "https://status.example.com/report" \
  -u h1 \
  -p p1
```

如果出于其它修复需要升级 Agent，可单独构建或替换 `stat_client`，但这不是历史数据或标签功能的依赖条件。

## 9. 后续建议

优先级较高的接续工作：

1. 实现 `HistoryStore` 的 VictoriaMetrics 和 InfluxDB 插件，并把 `create_store` 改成真正的后端注册/工厂机制。
2. 将同步 SQLite 查询移入阻塞线程池，增加查询超时、并发限制和正确 HTTP 状态码。
3. 为 `/api/history` 增加按部署需要配置的鉴权、限流和最大返回点数。
4. 增加标签管理后台、配置热加载、批量编辑和 AND/OR 多标签筛选；当前只有配置文件和单标签筛选。
5. 增加前端自动化测试，覆盖 IndexedDB 缺口计算、淘汰策略、刷新恢复、标签分组和多标签重复显示。
6. 增加 SQLite migration 框架、数据库损坏恢复说明、运行指标和写队列丢样计数。
7. 更新 Dockerfile 的 Rust 版本，并把前端构建正式纳入 Docker/CI 流程，避免依赖仓库中预生成的 `web/`。
8. 评估历史查询桶内 Gauge 使用最后值还是平均值；当前 API 实际使用最后值，指标目录中保留了未来聚合语义。
9. 对 IndexedDB 进行大量主机、30 天数据压力测试；当前 512 MiB 是应用层保守估算。

## 10. 重要文件索引

- `server/src/history.rs`：历史抽象、SQLite、查询、清理及测试。
- `server/src/config.rs`：历史配置、多标签配置与兼容测试。
- `server/src/stats.rs`：实时统计接入历史队列、离线点和标签覆盖。
- `server/src/http.rs`：`/api/history` handler。
- `server/src/main.rs`：历史模块及路由注册。
- `frontend/src/history.ts`：实时内存、IndexedDB、缺口补拉和指标定义。
- `frontend/src/App.vue`：Theme-Light 设置、时间序列、标签筛选和分组。
- `frontend/src/components/ServerCard.vue`：卡片标签展示和图表。
- `frontend/src/components/ServerItem.vue`：列表标签展示。
- `config.toml`：部署配置示例。
- `docker-compose.yml`：SQLite 数据卷。
- `README.md`：面向用户的功能和基本使用说明。

