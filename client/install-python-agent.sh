#!/bin/bash
# ServerStatus Python Agent 一键安装脚本
# 用法: sudo bash install_serverstatus_agent.sh -u <用户名> -p <密码> [-l <位置>] [-a <服务器地址>]

set -e

# 默认值
AG_LOC="cn"
AG_ADDR="https://st.9888812.xyz/report"
INSTALL_DIR="/opt/ServerStatus"
VENV_DIR="${INSTALL_DIR}/.venv"
CLIENT_PY="${INSTALL_DIR}/stat_client.py"
SERVICE_NAME="serverstatus-agent"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

# 参数解析
while getopts "u:p:l:a:h" opt; do
    case $opt in
        u) AG_USER="$OPTARG" ;;
        p) AG_PWD="$OPTARG" ;;
        l) AG_LOC="$OPTARG" ;;
        a) AG_ADDR="$OPTARG" ;;
        h)
            echo "用法: $0 -u <用户名> -p <密码> [-l <位置>] [-a <服务器地址>]"
            echo "  -u  节点用户名 (必填)"
            echo "  -p  节点密码   (必填)"
            echo "  -l  节点位置   (默认: cn)"
            echo "  -a  上报地址   (默认: https://st.9888812.xyz/report)"
            exit 0
            ;;
        *) error "无效参数，使用 -h 查看帮助" ;;
    esac
done

# 检查必填参数
if [ -z "$AG_USER" ] || [ -z "$AG_PWD" ]; then
    error "必须指定用户名和密码！使用 -h 查看帮助"
fi

# 检查 root 权限
if [ "$EUID" -ne 0 ]; then
    error "请使用 root 权限运行此脚本 (sudo bash $0 ...)"
fi

info "开始安装 ServerStatus Agent..."
info "用户名: $AG_USER | 位置: $AG_LOC | 地址: $AG_ADDR"

# 1. 安装系统依赖
info "检查并安装系统依赖..."
if command -v apt-get >/dev/null 2>&1; then
    apt-get update -qq
    apt-get install -y -qq python3 python3-venv python3-pip wget curl ca-certificates >/dev/null 2>&1 || true
elif command -v yum >/dev/null 2>&1; then
    yum install -y python3 python3-pip python3-devel gcc wget curl >/dev/null 2>&1 || true
elif command -v dnf >/dev/null 2>&1; then
    dnf install -y python3 python3-pip python3-devel gcc wget curl >/dev/null 2>&1 || true
else
    warn "无法自动安装系统依赖，请确保已安装 python3、python3-venv、wget"
fi

# 2. 创建目录
info "创建安装目录: $INSTALL_DIR"
mkdir -p "$INSTALL_DIR"
cd "$INSTALL_DIR"

# 3. 下载 stat_client.py
if [ ! -f "$CLIENT_PY" ]; then
    info "下载 stat_client.py..."
    wget --no-check-certificate -qO "$CLIENT_PY" \
        "https://raw.githubusercontent.com/8-diagrams/ServerStatusHistory/refs/heads/master/client/stat_client.py" \
        || error "下载 stat_client.py 失败，请检查网络"
    chmod +x "$CLIENT_PY"
else
    info "stat_client.py 已存在，跳过下载"
fi

# 4. 创建虚拟环境
if [ ! -d "$VENV_DIR" ]; then
    info "创建 Python 虚拟环境..."
    python3 -m venv "$VENV_DIR" || error "创建虚拟环境失败，请确认已安装 python3-venv"
else
    info "虚拟环境已存在"
fi

# 5. 安装 Python 依赖
info "安装 Python 依赖: psutil, py-cpuinfo, requests"
"$VENV_DIR/bin/pip" install --upgrade pip -q
"$VENV_DIR/bin/pip" install psutil py-cpuinfo requests -q || error "安装依赖失败"

# 6. 生成 systemd 服务文件
info "生成 systemd 服务文件..."
cat > "$SERVICE_FILE" << SERVICEEOF
[Unit]
Description=ServerStatus Python Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=${INSTALL_DIR}
ExecStart=${VENV_DIR}/bin/python -u ${CLIENT_PY} \\
  -a ${AG_ADDR} \\
  -u ${AG_USER} \\
  -p ${AG_PWD} \\
  --location ${AG_LOC} \\
  --type linux
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
SERVICEEOF

# 7. 重载并启动服务
info "重载 systemd 并启动服务..."
systemctl daemon-reload
systemctl enable "$SERVICE_NAME" >/dev/null 2>&1
systemctl restart "$SERVICE_NAME"

# 8. 检查状态
sleep 2
if systemctl is-active --quiet "$SERVICE_NAME"; then
    info "服务启动成功！"
    echo
    systemctl status "$SERVICE_NAME" --no-pager -l
else
    error "服务启动失败，请检查日志: journalctl -u $SERVICE_NAME -n 50"
fi

echo
info "安装完成！"
info "服务名称: $SERVICE_NAME"
info "安装目录: $INSTALL_DIR"
info "查看日志: journalctl -u $SERVICE_NAME -f"
info "停止服务: systemctl stop $SERVICE_NAME"
info "重启服务: systemctl restart $SERVICE_NAME"

