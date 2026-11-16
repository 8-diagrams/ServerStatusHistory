#!/usr/bin/env bash
# ServerStatus Rust Agent installer
# Installs stat_client from a local build, local ZIP, or GitHub Release URL.

set -Eeuo pipefail

PROGRAM_NAME="$(basename "$0")"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

INSTALL_DIR="/opt/ServerStatus"
SERVICE_NAME="stat_client"
SERVICE_FILE=""
ENV_FILE=""
BINARY_FILE=""
ARTIFACT=""

AG_ADDR="https://st.9888812.xyz/report"
AG_USER=""
AG_PWD=""
AG_LOC="cn"
AG_TYPE="linux"
AG_GID=""
AG_ALIAS="unknown"
AG_INTERVAL="1"
START_SERVICE=true

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { printf '%b[INFO]%b %s\n' "$GREEN" "$NC" "$*"; }
warn() { printf '%b[WARN]%b %s\n' "$YELLOW" "$NC" "$*"; }
fail() { printf '%b[ERROR]%b %s\n' "$RED" "$NC" "$*" >&2; exit 1; }

usage() {
    cat <<EOF
用法:
  sudo ./${PROGRAM_NAME} --artifact <本地路径或HTTPS地址> -u <用户名> -p <密码> [选项]
  sudo ./${PROGRAM_NAME} -g <组ID> -p <组密码> [选项]

二进制:
  -b, --artifact <来源>     stat_client、ServerStatus-*.zip 或 HTTPS 下载地址
                            不指定时先查找本地产物，否则下载 master Release

节点配置:
  -u, --user <用户名>       固定主机用户名；非组模式必填
  -p, --password <密码>     固定主机密码或组密码（必填）
  -g, --gid <组ID>          动态主机组 ID
      --alias <名称>        动态主机展示名称（默认: unknown）
  -l, --location <位置>     节点位置（默认: cn）
  -a, --addr <地址>         上报地址（默认: ${AG_ADDR}）
  -t, --type <类型>         节点类型（默认: linux）
  -i, --interval <秒>       上报间隔（默认: 1）

安装选项:
      --install-dir <目录>  安装目录（默认: ${INSTALL_DIR}）
      --service <名称>      systemd 服务名（默认: ${SERVICE_NAME}）
      --no-start            只安装，不启动服务
  -h, --help                显示帮助

示例:
  sudo ./${PROGRAM_NAME} --artifact ./stat_client \
    -u mwh-1 -p 'agent-password' -l us -a https://st.example.com/report

  sudo ./${PROGRAM_NAME} --artifact ./ServerStatus-x86_64-unknown-linux-musl.zip \
    -g mail -p 'group-password' --alias mail-01 -l us

  sudo ./${PROGRAM_NAME} --artifact \
    https://github.com/8-diagrams/ServerStatusHistory/releases/download/master/ServerStatus-x86_64-unknown-linux-musl.zip \
    -u mwh-1 -p 'agent-password' -l us
EOF
}

need_value() {
    [[ $# -ge 2 && -n "$2" ]] || fail "$1 需要参数值"
}

while (($# > 0)); do
    case "$1" in
        -b|--artifact)
            need_value "$@"
            ARTIFACT="$2"
            shift 2
            ;;
        -u|--user)
            need_value "$@"
            AG_USER="$2"
            shift 2
            ;;
        -p|--password|--pass)
            need_value "$@"
            AG_PWD="$2"
            shift 2
            ;;
        -g|--gid)
            need_value "$@"
            AG_GID="$2"
            shift 2
            ;;
        --alias)
            need_value "$@"
            AG_ALIAS="$2"
            shift 2
            ;;
        -l|--location)
            need_value "$@"
            AG_LOC="$2"
            shift 2
            ;;
        -a|--addr)
            need_value "$@"
            AG_ADDR="$2"
            shift 2
            ;;
        -t|--type)
            need_value "$@"
            AG_TYPE="$2"
            shift 2
            ;;
        -i|--interval)
            need_value "$@"
            AG_INTERVAL="$2"
            shift 2
            ;;
        --install-dir)
            need_value "$@"
            INSTALL_DIR="$2"
            shift 2
            ;;
        --service)
            need_value "$@"
            SERVICE_NAME="$2"
            shift 2
            ;;
        --no-start)
            START_SERVICE=false
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            fail "未知参数: $1（使用 --help 查看帮助）"
            ;;
    esac
done

[[ "$(id -u)" -eq 0 ]] || fail "请使用 root 权限运行（sudo bash ${PROGRAM_NAME} ...）"
[[ "$(uname -s)" == "Linux" ]] || fail "Rust Agent 安装脚本目前仅支持 Linux"
command -v systemctl >/dev/null 2>&1 || fail "找不到 systemctl；当前系统需要使用 systemd"

[[ -n "$AG_PWD" ]] || fail "必须指定 -p/--password"
if [[ -z "$AG_GID" && -z "$AG_USER" ]]; then
    fail "固定主机必须指定 -u/--user；动态组请指定 -g/--gid"
fi
if [[ -n "$AG_GID" && -z "$AG_USER" ]]; then
    AG_USER="h1"
fi
[[ "$AG_INTERVAL" =~ ^[1-9][0-9]*$ ]] || fail "--interval 必须是大于 0 的整数"
[[ "$SERVICE_NAME" =~ ^[A-Za-z0-9_.@-]+$ ]] || fail "systemd 服务名包含无效字符"
[[ "$INSTALL_DIR" != *[[:space:]]* ]] || fail "--install-dir 暂不支持包含空白字符的路径"

for value_name in AG_ADDR AG_USER AG_PWD AG_LOC AG_TYPE AG_GID AG_ALIAS; do
    value="${!value_name}"
    [[ "$value" != *$'\n'* && "$value" != *$'\r'* ]] || fail "配置值不能包含换行符: ${value_name}"
done

case "$(uname -m)" in
    x86_64|amd64)
        TARGET="x86_64-unknown-linux-musl"
        ;;
    aarch64|arm64)
        TARGET="aarch64-unknown-linux-musl"
        ;;
    *)
        fail "暂不支持当前架构: $(uname -m)；请通过 --artifact 提供兼容二进制并扩展脚本"
        ;;
esac

if [[ -z "$ARTIFACT" ]]; then
    candidates=(
        "$(pwd)/stat_client"
        "${SCRIPT_DIR}/stat_client"
        "$(pwd)/target/${TARGET}/release/stat_client"
        "${SCRIPT_DIR}/../target/${TARGET}/release/stat_client"
    )
    for candidate in "${candidates[@]}"; do
        if [[ -f "$candidate" ]]; then
            ARTIFACT="$candidate"
            break
        fi
    done
    if [[ -z "$ARTIFACT" ]]; then
        ARTIFACT="https://github.com/8-diagrams/ServerStatusHistory/releases/download/master/ServerStatus-${TARGET}.zip"
        info "未找到本地构建产物，将下载 master Release"
    fi
fi

STAGE_DIR="$(mktemp -d /tmp/serverstatus-rust-agent.XXXXXX)"
cleanup() {
    rm -rf "$STAGE_DIR"
}
trap cleanup EXIT

case "$ARTIFACT" in
    https://*)
        DOWNLOAD_FILE="${STAGE_DIR}/ServerStatus-${TARGET}.zip"
        info "下载构建产物: $ARTIFACT"
        if command -v curl >/dev/null 2>&1; then
            curl -fL \
                --retry 3 \
                --retry-delay 2 \
                --connect-timeout 15 \
                -o "$DOWNLOAD_FILE" \
                "$ARTIFACT" \
                || fail "下载失败: $ARTIFACT"
        elif command -v wget >/dev/null 2>&1; then
            wget -O "$DOWNLOAD_FILE" "$ARTIFACT" \
                || fail "下载失败: $ARTIFACT"
        else
            fail "下载 Release 需要 curl 或 wget"
        fi
        [[ -s "$DOWNLOAD_FILE" ]] || fail "下载文件为空: $ARTIFACT"
        ARTIFACT="$DOWNLOAD_FILE"
        ;;
    http://*)
        fail "远程构建产物只允许使用 HTTPS 地址"
        ;;
    *)
        [[ -f "$ARTIFACT" ]] || fail "构建产物不存在: $ARTIFACT"
        ARTIFACT="$(cd "$(dirname "$ARTIFACT")" && pwd)/$(basename "$ARTIFACT")"
        ;;
esac

case "$ARTIFACT" in
    *.zip)
        command -v unzip >/dev/null 2>&1 || fail "安装 ZIP 需要 unzip，请先安装 unzip"
        info "解压构建产物: $ARTIFACT"
        unzip -q "$ARTIFACT" -d "$STAGE_DIR"
        BINARY_SOURCE="$(find "$STAGE_DIR" -type f -name stat_client -print -quit)"
        if [[ -z "$BINARY_SOURCE" ]]; then
            NESTED_ZIP="$(find "$STAGE_DIR" -type f -name '*.zip' -print -quit)"
            if [[ -n "$NESTED_ZIP" ]]; then
                mkdir -p "$STAGE_DIR/nested"
                unzip -q "$NESTED_ZIP" -d "$STAGE_DIR/nested"
                BINARY_SOURCE="$(find "$STAGE_DIR/nested" -type f -name stat_client -print -quit)"
            fi
        fi
        [[ -n "$BINARY_SOURCE" ]] || fail "ZIP 中找不到 stat_client"
        ;;
    *)
        BINARY_SOURCE="$ARTIFACT"
        ;;
esac

VALIDATED_BINARY="${STAGE_DIR}/stat_client.validated"
install -m 0755 "$BINARY_SOURCE" "$VALIDATED_BINARY"
if ! VERSION_OUTPUT="$($VALIDATED_BINARY --version 2>&1)"; then
    fail "stat_client 无法在当前主机运行，可能是架构不匹配: $VERSION_OUTPUT"
fi
[[ "$VERSION_OUTPUT" == stat_client* ]] || fail "指定文件不是 stat_client: $VERSION_OUTPUT"
BINARY_SOURCE="$VALIDATED_BINARY"

SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
ENV_FILE="${INSTALL_DIR}/stat_client.env"
BINARY_FILE="${INSTALL_DIR}/stat_client"

systemd_env_escape() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    printf '%s' "$value"
}

info "检测到架构: $(uname -m) (${TARGET})"
info "构建版本: $VERSION_OUTPUT"
info "安装目录: $INSTALL_DIR"
info "上报地址: $AG_ADDR"
if [[ -n "$AG_GID" ]]; then
    info "节点模式: 动态组 gid=$AG_GID alias=$AG_ALIAS location=$AG_LOC"
else
    info "节点模式: 固定主机 user=$AG_USER location=$AG_LOC"
fi

if [[ "$SERVICE_NAME" != "serverstatus-agent" ]] \
    && systemctl is-active --quiet serverstatus-agent; then
    warn "检测到 Python Agent 服务 serverstatus-agent 正在运行"
    warn "请确认后停止它，避免 Python/Rust Agent 使用同一身份重复上报：systemctl disable --now serverstatus-agent"
fi

mkdir -p "$INSTALL_DIR"
if systemctl is-active --quiet "$SERVICE_NAME"; then
    info "停止现有服务: $SERVICE_NAME"
    systemctl stop "$SERVICE_NAME"
fi

if [[ -f "$BINARY_FILE" ]]; then
    cp -p "$BINARY_FILE" "${BINARY_FILE}.previous"
    info "旧二进制已备份到 ${BINARY_FILE}.previous"
fi
install -m 0755 "$BINARY_SOURCE" "${BINARY_FILE}.new"
mv -f "${BINARY_FILE}.new" "$BINARY_FILE"

{
    printf 'SSR_ADDR="%s"\n' "$(systemd_env_escape "$AG_ADDR")"
    printf 'SSR_USER="%s"\n' "$(systemd_env_escape "$AG_USER")"
    printf 'SSR_PASS="%s"\n' "$(systemd_env_escape "$AG_PWD")"
    printf 'SSR_LOC="%s"\n' "$(systemd_env_escape "$AG_LOC")"
    printf 'SSR_TYPE="%s"\n' "$(systemd_env_escape "$AG_TYPE")"
    printf 'SSR_INTERVAL="%s"\n' "$AG_INTERVAL"
    printf 'SSR_GID="%s"\n' "$(systemd_env_escape "$AG_GID")"
    printf 'SSR_ALIAS="%s"\n' "$(systemd_env_escape "$AG_ALIAS")"
} > "${ENV_FILE}.new"
chmod 0600 "${ENV_FILE}.new"
mv -f "${ENV_FILE}.new" "$ENV_FILE"

cat > "${SERVICE_FILE}.new" <<EOF
[Unit]
Description=ServerStatus Rust Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=${INSTALL_DIR}
Environment="RUST_BACKTRACE=1"
Environment="RUST_LOG=info"
EnvironmentFile=${ENV_FILE}
ExecStart=${BINARY_FILE}
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
chmod 0644 "${SERVICE_FILE}.new"
mv -f "${SERVICE_FILE}.new" "$SERVICE_FILE"

systemctl daemon-reload

if [[ "$START_SERVICE" == true ]]; then
    info "启动服务: $SERVICE_NAME"
    systemctl enable "$SERVICE_NAME" >/dev/null 2>&1
    systemctl restart "$SERVICE_NAME"
    sleep 2
    if ! systemctl is-active --quiet "$SERVICE_NAME"; then
        systemctl status "$SERVICE_NAME" --no-pager -l || true
        fail "服务启动失败，请检查: journalctl -u ${SERVICE_NAME} -n 100 --no-pager"
    fi
    systemctl status "$SERVICE_NAME" --no-pager -l
else
    warn "已按 --no-start 跳过服务启动"
fi

echo
info "Rust Agent 安装完成"
info "服务名称: $SERVICE_NAME"
info "二进制: $BINARY_FILE"
info "配置文件: $ENV_FILE（权限 0600）"
info "查看日志: journalctl -u $SERVICE_NAME -f"
info "重启服务: systemctl restart $SERVICE_NAME"
