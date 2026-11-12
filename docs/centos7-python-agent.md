# CentOS 7 部署 Python Agent

本文记录 CentOS 7、Python 3.6 环境部署 `client/stat_client.py` 的流程。

示例参数：

```text
Server: https://st.9888812.xyz
Agent name: mwh-1
Agent password: agent863
Install directory: /opt/ServerStatus
```

Python Agent 使用 HTTP JSON 上报，不需要修改现有 Agent 协议，也不需要上报标签。标签由 Server 的 `config.toml` 管理。

## 1. Server 注册节点

在 Server 上编辑：

```bash
sudo nano /opt/ServerStatus/config.toml
```

增加固定节点：

```toml
hosts = [
  {name = "mwh-1", password = "agent863", alias = "MWH-1", location = "us", tags = ["AWS", "MAILSVR"]},
]
```

对应关系：

```text
hosts.name     → Python Agent -u
hosts.password → Python Agent -p
hosts.alias    → Web 展示名称
hosts.tags     → Web 标签分组
```

检查并重启 Server：

```bash
sudo /opt/ServerStatus/stat_server -c /opt/ServerStatus/config.toml -t
sudo systemctl restart stat_server
```

## 2. 安装 CentOS 7 依赖

CentOS 7 已停止维护，建议后续迁移到 Rocky Linux、AlmaLinux 或其他受支持系统。当前环境可以执行：

```bash
sudo yum install -y epel-release
sudo yum install -y python3 python3-pip python3-devel gcc
```

检查版本：

```bash
python3 --version
```

Python Agent 使用了 f-string，必须使用 Python 3.6 或更高版本。不要使用可能指向 Python 2 的：

```bash
python stat_client.py
```

## 3. 安装 Python Agent

从开发机上传脚本：

```bash
scp /Users/cxx/vibesrc/ServerStatus-Rust/client/stat_client.py \
  root@Agent服务器IP:/tmp/
```

在 Agent 服务器执行：

```bash
sudo mkdir -p /opt/ServerStatus
sudo install -m 0755 /tmp/stat_client.py /opt/ServerStatus/stat_client.py
cd /opt/ServerStatus
python3 -m venv .venv
```

如果缺少 `venv`，可以使用：

```bash
python3 -m pip install --user virtualenv
python3 -m virtualenv /opt/ServerStatus/.venv
```

后续安装依赖和运行脚本都必须使用：

```text
/opt/ServerStatus/.venv/bin/python
```

## 4. Python 3.6 的 pip 和 locale 处理

CentOS 7 常见 pip 版本很旧，例如 `pip 9.0.3`。Python 3.6 不能使用最新 pip，因此固定到兼容版本：

```bash
cd /opt/ServerStatus

export LANG=C
export LC_ALL=C

.venv/bin/python -m pip install --upgrade \
  "pip==21.3.1" \
  "setuptools==59.6.0" \
  wheel
```

如果看到：

```text
setlocale: LC_ALL: cannot change locale (C.UTF-8)
```

说明系统没有 `C.UTF-8`，使用上面的 `LANG=C`、`LC_ALL=C` 即可。该警告本身不是 Agent 故障原因。

安装 Python 3.6 兼容依赖：

```bash
.venv/bin/python -m pip install \
  --only-binary=:all: \
  "psutil==5.9.8" \
  "requests==2.27.1" \
  "py-cpuinfo==9.0.0"
```

版本说明：

- `psutil==5.9.8`：提供 Python 3.6 可用的 manylinux wheel。
- `requests==2.27.1`：兼容 Python 3.6。
- `py-cpuinfo==9.0.0`：当前软件源中可用的兼容版本。
- `py-cpuinfo==8.0.0` 在本次软件源中不可用，不要固定该版本。

验证：

```bash
.venv/bin/python -c \
  "import psutil, requests, cpuinfo; print('Python Agent dependencies OK')"
```

如果 psutil 仍尝试源码编译：

```bash
sudo yum install -y gcc python3-devel
```

## 5. 前台测试

```bash
cd /opt/ServerStatus

.venv/bin/python stat_client.py \
  -a "https://st.9888812.xyz/report" \
  -u "mwh-1" \
  -p "agent863" \
  --location us \
  --type linux
```

Python Agent 本身就是 JSON HTTP 上报，不要添加 Rust Agent 的 `--json` 参数。正常时会持续输出采集指标，使用 `Ctrl+C` 停止测试。

## 6. 配置 systemd

创建：

```bash
sudo nano /etc/systemd/system/stat_client.service
```

写入：

```ini
[Unit]
Description=ServerStatus Python Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=/opt/ServerStatus
Environment=PYTHONUNBUFFERED=1
ExecStart=/opt/ServerStatus/.venv/bin/python -u /opt/ServerStatus/stat_client.py \
  -a https://st.9888812.xyz/report \
  -u mwh-1 \
  -p agent863 \
  --location us \
  --type linux
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

保护服务文件并启动：

```bash
sudo chmod 0600 /etc/systemd/system/stat_client.service
sudo systemctl daemon-reload
sudo systemctl enable --now stat_client
```

查看状态和日志：

```bash
sudo systemctl status stat_client
sudo journalctl -u stat_client -f
```

## 7. 验证节点

在 Server 或其他机器执行：

```bash
curl -fsS https://st.9888812.xyz/json/stats.json
```

应看到类似：

```json
{"name":"mwh-1","alias":"MWH-1","tags":["AWS","MAILSVR"]}
```

打开 `https://st.9888812.xyz`，可以在设置中按标签筛选或按标签分组。

## 8. 动态主机组

Server 配置：

```toml
hosts_group = [
  {gid = "mail", password = "mail-group-password", tags = ["AWS", "MAILSVR"]},
]
```

systemd 的 `ExecStart` 改为：

```ini
ExecStart=/opt/ServerStatus/.venv/bin/python -u /opt/ServerStatus/stat_client.py \
  -a https://st.9888812.xyz/report \
  -g mail \
  -p mail-group-password \
  --alias mwh-1 \
  --location us \
  --type linux
```

动态组中：

- `-g` 对应 `hosts_group.gid`。
- `-p` 对应组密码。
- `--alias` 是 Web 展示名称。
- 所有动态节点继承组标签。

## 9. 常见故障

### `SyntaxError` 出现在 f-string

检查：

```bash
python --version
python3 --version
/opt/ServerStatus/.venv/bin/python --version
```

运行时必须使用虚拟环境的 Python 3.6+。

### `No module named psutil`

说明依赖装到了其他 Python。使用同一个解释器：

```bash
/opt/ServerStatus/.venv/bin/python -m pip install \
  "psutil==5.9.8" \
  "requests==2.27.1" \
  "py-cpuinfo==9.0.0"
```

### HTTP 401

固定节点必须满足：

```text
-u == hosts.name
-p == hosts.password
```

动态组必须满足：

```text
-g == hosts_group.gid
-p == hosts_group.password
```

修改 Server 配置后：

```bash
sudo systemctl restart stat_server
```

### Server 页面没有节点

Agent 端：

```bash
sudo journalctl -u stat_client -n 100 --no-pager
curl -I https://st.9888812.xyz/report
```

Server 端：

```bash
sudo journalctl -u stat_server -n 100 --no-pager
curl -fsS http://127.0.0.1:8080/json/stats.json
```

### 降低资源消耗

在命令中增加：

```text
--disable-extra   关闭系统额外信息
--disable-ping    关闭三网延迟和丢包探测
--disable-tupd    关闭 TCP/UDP/进程/线程统计
```

### 使用 vnstat

```bash
sudo yum install -y vnstat
sudo systemctl enable --now vnstat
```

Agent 参数增加：

```text
--vnstat
```

### HTTP 上报超时与 520/5xx 排查

当前 Python Agent 将单次 HTTP 上报的总墙钟时间限制为 8 秒：连接超时 3 秒、读取超时 5 秒；CentOS/Linux 另外使用 `SIGALRM` 强制执行 8 秒总截止时间。请求超时或 HTTP 4xx/5xx 后会关闭旧 Session，等待 3～6 秒并重新连接，避免一次 Cloudflare 520 或卡住的连接让整个采集循环停止几十秒。

正常响应日志示例：

```text
report status=200 elapsed=0.842s cf_ray=...
```

超时日志示例：

```text
report timeout: report request exceeded 8.0s total timeout
```

Agent 还会上报 `latest_ts` 客户端采样时间；新版服务端会将它记录为 `agent_ts`，用于区分客户端采集停顿、网络传输延迟和服务端内部排队。旧版服务端会忽略这个兼容字段，不影响认证和指标上报。

## 10. 更新 Agent

```bash
sudo systemctl stop stat_client
sudo install -m 0755 ./stat_client.py /opt/ServerStatus/stat_client.py
sudo systemctl start stat_client
sudo systemctl status stat_client
```

依赖未变化时不需要重建 `.venv`。依赖变化时，使用第 4 节的固定版本命令重新安装。

