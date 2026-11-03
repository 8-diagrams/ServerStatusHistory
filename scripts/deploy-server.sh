#!/usr/bin/env bash

set -Eeuo pipefail

PROGRAM_NAME="$(basename "$0")"
INSTALL_DIR="/opt/ServerStatus"
SERVICE_NAME="stat_server"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
ARTIFACT=""
CONFIG_SOURCE=""
START_SERVICE=true
REGENERATE_AUTH=false
ADMIN_USER=""
NEW_ADMIN_PASSWORD=""

log() {
    printf '[deploy] %s\n' "$*"
}

fail() {
    printf '[deploy] ERROR: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat <<EOF
Usage:
  sudo $PROGRAM_NAME --artifact <stat_server-or-zip> [options]

Required:
  --artifact <path>      stat_server binary or GitHub Actions ZIP artifact

Options:
  --config <path>        Install this config.toml. If omitted, preserve the
                         installed config or use config.toml from the ZIP.
  --install-dir <path>   Installation directory (default: /opt/ServerStatus)
  --regenerate-auth      Generate a new admin password and JWT secret before
                         restart; admin_user is generated as well.
  --admin-user <name>    Use this username instead of generating one
  --no-start             Install files but do not enable/start the service
  -h, --help             Show this help

Examples:
  sudo $PROGRAM_NAME --artifact ./stat_server-x86_64-unknown-linux-musl.zip
  sudo $PROGRAM_NAME --artifact ./stat_server --config ./config.toml
EOF
}

while (($# > 0)); do
    case "$1" in
        --artifact)
            (($# >= 2)) || fail "--artifact requires a path"
            ARTIFACT="$2"
            shift 2
            ;;
        --config)
            (($# >= 2)) || fail "--config requires a path"
            CONFIG_SOURCE="$2"
            shift 2
            ;;
        --install-dir)
            (($# >= 2)) || fail "--install-dir requires a path"
            INSTALL_DIR="$2"
            shift 2
            ;;
        --regenerate-auth)
            REGENERATE_AUTH=true
            shift
            ;;
        --admin-user)
            (($# >= 2)) || fail "--admin-user requires a name"
            ADMIN_USER="$2"
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
            fail "unknown argument: $1"
            ;;
    esac
done

[[ "$(id -u)" -eq 0 ]] || fail "run this script as root (use sudo)"
[[ "$(uname -s)" == "Linux" ]] || fail "this deployment script only supports Linux"
command -v systemctl >/dev/null 2>&1 || fail "systemd/systemctl is required"
[[ -n "$ARTIFACT" ]] || fail "--artifact is required"
[[ -f "$ARTIFACT" ]] || fail "artifact not found: $ARTIFACT"
if [[ -n "$ADMIN_USER" && ! "$ADMIN_USER" =~ ^[A-Za-z0-9._-]+$ ]]; then
    fail "--admin-user may contain only letters, numbers, dot, underscore and hyphen"
fi
if [[ -n "$CONFIG_SOURCE" ]]; then
    [[ -f "$CONFIG_SOURCE" ]] || fail "config not found: $CONFIG_SOURCE"
fi

STAGE_DIR="$(mktemp -d /tmp/serverstatus-deploy.XXXXXX)"
cleanup() {
    rm -rf "$STAGE_DIR"
}
trap cleanup EXIT

case "$ARTIFACT" in
    *.zip)
        command -v unzip >/dev/null 2>&1 || fail "unzip is required to deploy a ZIP artifact"
        log "extracting $ARTIFACT"
        unzip -q "$ARTIFACT" -d "$STAGE_DIR"
        BINARY_SOURCE="$(find "$STAGE_DIR" -type f -name stat_server -print -quit)"
        if [[ -z "$BINARY_SOURCE" ]]; then
            NESTED_ZIP="$(find "$STAGE_DIR" -type f -name '*.zip' -print -quit)"
            if [[ -n "$NESTED_ZIP" ]]; then
                log "extracting nested GitHub Actions ZIP: $NESTED_ZIP"
                mkdir -p "$STAGE_DIR/nested"
                unzip -q "$NESTED_ZIP" -d "$STAGE_DIR/nested"
                BINARY_SOURCE="$(find "$STAGE_DIR/nested" -type f -name stat_server -print -quit)"
            fi
        fi
        [[ -n "$BINARY_SOURCE" ]] || fail "ZIP does not contain stat_server"
        if [[ -z "$CONFIG_SOURCE" ]]; then
            ARCHIVE_CONFIG="$(find "$STAGE_DIR" -type f -name config.toml -print -quit)"
        fi
        ;;
    *)
        BINARY_SOURCE="$ARTIFACT"
        ARCHIVE_CONFIG=""
        ;;
esac

chmod +x "$BINARY_SOURCE"
"$BINARY_SOURCE" --version >/dev/null 2>&1 || fail \
    "stat_server cannot run on this machine; check x86_64/aarch64 architecture"

mkdir -p "$INSTALL_DIR" "$INSTALL_DIR/data" "$INSTALL_DIR/backups"
touch "$INSTALL_DIR/stats.json"

if [[ -n "$CONFIG_SOURCE" ]]; then
    SELECTED_CONFIG="$CONFIG_SOURCE"
elif [[ -f "$INSTALL_DIR/config.toml" ]]; then
    SELECTED_CONFIG="$INSTALL_DIR/config.toml"
    log "preserving installed config: $SELECTED_CONFIG"
elif [[ -n "${ARCHIVE_CONFIG:-}" && -f "$ARCHIVE_CONFIG" ]]; then
    SELECTED_CONFIG="$ARCHIVE_CONFIG"
    log "using config.toml from the ZIP artifact"
    log "IMPORTANT: review host passwords and admin credentials after deployment"
else
    fail "first deployment requires --config or a ZIP containing config.toml"
fi

"$BINARY_SOURCE" -c "$SELECTED_CONFIG" -t >/dev/null || fail \
    "configuration validation failed: $SELECTED_CONFIG"

if [[ "$REGENERATE_AUTH" == true ]]; then
    command -v openssl >/dev/null 2>&1 || fail "openssl is required for --regenerate-auth"
    AUTH_CONFIG="$STAGE_DIR/config.toml"
    cp "$SELECTED_CONFIG" "$AUTH_CONFIG"
    if [[ -z "$ADMIN_USER" ]]; then
        ADMIN_USER="admin_$(openssl rand -hex 4)"
    fi
    NEW_ADMIN_PASSWORD="$(openssl rand -hex 24)"
    NEW_JWT_SECRET="$(openssl rand -hex 32)"

    set_config_value() {
        local key="$1"
        local value="$2"
        if grep -qE "^[[:space:]]*${key}[[:space:]]*=" "$AUTH_CONFIG"; then
            sed -i -E "s|^[[:space:]]*${key}[[:space:]]*=.*$|${key} = \"${value}\"|" "$AUTH_CONFIG"
        else
            sed -i "1i\\${key} = \"${value}\"" "$AUTH_CONFIG"
        fi
    }

    set_config_value "admin_user" "$ADMIN_USER"
    set_config_value "admin_pass" "$NEW_ADMIN_PASSWORD"
    set_config_value "jwt_secret" "$NEW_JWT_SECRET"
    SELECTED_CONFIG="$AUTH_CONFIG"
    "$BINARY_SOURCE" -c "$SELECTED_CONFIG" -t >/dev/null || fail \
        "generated authentication configuration failed validation"
    log "generated new admin credentials and JWT secret"
fi

TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="$INSTALL_DIR/backups/$TIMESTAMP"
mkdir -p "$BACKUP_DIR"

if [[ -f "$SERVICE_FILE" ]] || systemctl is-active --quiet "$SERVICE_NAME"; then
    log "stopping existing $SERVICE_NAME service"
    systemctl stop "$SERVICE_NAME" || true
fi

for existing_file in \
    "$INSTALL_DIR/stat_server" \
    "$INSTALL_DIR/config.toml" \
    "$INSTALL_DIR/stats.json" \
    "$SERVICE_FILE"; do
    if [[ -f "$existing_file" ]]; then
        cp -a "$existing_file" "$BACKUP_DIR/"
    fi
done

log "installing stat_server into $INSTALL_DIR"
install -m 0755 "$BINARY_SOURCE" "$INSTALL_DIR/stat_server.new"
mv -f "$INSTALL_DIR/stat_server.new" "$INSTALL_DIR/stat_server"

if [[ "$SELECTED_CONFIG" != "$INSTALL_DIR/config.toml" ]]; then
    install -m 0640 "$SELECTED_CONFIG" "$INSTALL_DIR/config.toml.new"
    mv -f "$INSTALL_DIR/config.toml.new" "$INSTALL_DIR/config.toml"
fi

cat >"$SERVICE_FILE" <<EOF
[Unit]
Description=ServerStatus-Rust Server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
Group=root
Environment="RUST_BACKTRACE=1"
WorkingDirectory=$INSTALL_DIR
ExecStart=$INSTALL_DIR/stat_server -c $INSTALL_DIR/config.toml
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
EOF

chmod 0644 "$SERVICE_FILE"
systemctl daemon-reload

if [[ "$START_SERVICE" == true ]]; then
    log "enabling and starting $SERVICE_NAME"
    systemctl enable "$SERVICE_NAME" >/dev/null
    systemctl restart "$SERVICE_NAME"
    sleep 1

    if ! systemctl is-active --quiet "$SERVICE_NAME"; then
        systemctl status "$SERVICE_NAME" --no-pager || true
        fail "service failed to start; backup is available at $BACKUP_DIR"
    fi

    log "deployment completed"
    systemctl status "$SERVICE_NAME" --no-pager --lines=5 || true
else
    log "files installed; service was not started (--no-start)"
fi

log "install directory: $INSTALL_DIR"
log "backup directory: $BACKUP_DIR"
log "config file: $INSTALL_DIR/config.toml"
log "history directory: $INSTALL_DIR/data"
if [[ -n "$NEW_ADMIN_PASSWORD" ]]; then
    log "new admin username: $ADMIN_USER"
    log "new admin password: $NEW_ADMIN_PASSWORD"
    log "save this password now; it is not shown again"
fi
