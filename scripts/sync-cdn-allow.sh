#!/bin/bash
# Generate Cloudflare and Bunny.net allow/deny config files
# Only overwrite if download is valid and non-empty
set -euo pipefail

detect_nginx_root() {
    local prefix=""
    if command -v nginx >/dev/null 2>&1; then
        prefix=$(nginx -V 2>&1 | grep -oP -- '--prefix=\K[^ ]+' || true)
    fi
    if [[ -z "$prefix" ]]; then
        local conf
        conf=$(nginx -t 2>&1 | grep -oP 'configuration file \K[^ ]+' || true)
        [[ -n "$conf" ]] && prefix=$(dirname "$conf")
    fi
    if [[ -z "$prefix" ]]; then
        for p in /etc/nginx /usr/local/nginx /usr/local/etc/nginx; do
            [[ -f "$p/nginx.conf" ]] && { prefix="$p"; break; }
        done
    fi
    [[ -z "$prefix" ]] && { echo "Cannot detect nginx root" >&2; exit 1; }
    echo "$prefix"
}

# Validate IP list more loosely
validate_ip_list() {
    local file="$1"
    local min_lines="${2:-5}"
    local count

    [[ -f "$file" ]] || { echo "  File not found: $file" >&2; return 1; }

    # Count lines that contain at least one digit and look roughly like an IP
    count=$(grep -cE '[0-9]' "$file" || true)

    if [[ "$count" -lt "$min_lines" ]]; then
        echo "  Only $count lines with digits in $file (need >= $min_lines)" >&2
        echo "  File size: $(wc -c < "$file") bytes" >&2
        echo "  First few lines:" >&2
        head -5 "$file" | cat -A >&2
        return 1
    fi
    return 0
}

NGINX_ROOT=$(detect_nginx_root)
CF_FILE="${NGINX_ROOT}/cdn-cloudflare-allow.conf"
BUNNY_FILE="${NGINX_ROOT}/cdn-bunny-allow.conf"

echo "Nginx root: $NGINX_ROOT"
echo "Cloudflare allow: $CF_FILE"
echo "Bunny.net allow:  $BUNNY_FILE"
echo

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading IP lists..."

download_ok=true
curl -fsSL -o "$TMP_DIR/cf-v4.txt" "https://www.cloudflare.com/ips-v4" || { echo "Failed: Cloudflare v4"; download_ok=false; }
curl -fsSL -o "$TMP_DIR/cf-v6.txt" "https://www.cloudflare.com/ips-v6" || { echo "Failed: Cloudflare v6"; download_ok=false; }
curl -fsSL -o "$TMP_DIR/bunny-v4.txt" "https://bunnycdn.com/api/system/edgeserverlist/plain" || { echo "Failed: Bunny v4"; download_ok=false; }
curl -fsSL -o "$TMP_DIR/bunny-v6.txt" "https://bunnycdn.com/api/system/edgeserverlist/IPv6/plain" || { echo "Failed: Bunny v6"; download_ok=false; }

if [[ "$download_ok" != true ]]; then
    echo "ERROR: One or more downloads failed. Existing files not overwritten." >&2
    exit 1
fi

echo "Validating downloads..."

if ! validate_ip_list "$TMP_DIR/cf-v4.txt" 10; then
    echo "ERROR: Cloudflare v4 list invalid. Existing files not overwritten." >&2
    exit 1
fi
if ! validate_ip_list "$TMP_DIR/cf-v6.txt" 3; then
    echo "ERROR: Cloudflare v6 list invalid. Existing files not overwritten." >&2
    exit 1
fi

if ! validate_ip_list "$TMP_DIR/bunny-v4.txt" 20; then
    echo "ERROR: Bunny.net v4 list invalid. Existing files not overwritten." >&2
    exit 1
fi
if ! validate_ip_list "$TMP_DIR/bunny-v6.txt" 10; then
    echo "ERROR: Bunny.net v6 list invalid. Existing files not overwritten." >&2
    exit 1
fi

echo "All lists validated. Generating config files..."

# Cloudflare allow
CF_TMP="${TMP_DIR}/cf-allow.conf"
{
    echo "# Cloudflare IP ranges (allow only)"
    echo "# Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo ""
    echo "# IPv4"
    while IFS= read -r line || [[ -n "$line" ]]; do
        line=$(echo "$line" | tr -d '[:space:]')
        [[ -z "$line" || "$line" =~ ^# ]] && continue
        echo "allow $line;"
    done < "$TMP_DIR/cf-v4.txt"
    echo ""
    echo "# IPv6"
    while IFS= read -r line || [[ -n "$line" ]]; do
        line=$(echo "$line" | tr -d '[:space:]')
        [[ -z "$line" || "$line" =~ ^# ]] && continue
        echo "allow $line;"
    done < "$TMP_DIR/cf-v6.txt"
    echo ""
    echo "deny all;"
} > "$CF_TMP"

# Bunny.net allow (add /32 and /128)
BUNNY_TMP="${TMP_DIR}/bunny-allow.conf"
{
    echo "# Bunny.net IP ranges (allow only)"
    echo "# Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')"
    echo ""
    echo "# IPv4"
    while IFS= read -r line || [[ -n "$line" ]]; do
        line=$(echo "$line" | tr -d '[:space:]')
        [[ -z "$line" || "$line" =~ ^# ]] && continue
        [[ "$line" != */* ]] && line="${line}/32"
        echo "allow $line;"
    done < "$TMP_DIR/bunny-v4.txt"
    echo ""
    echo "# IPv6"
    while IFS= read -r line || [[ -n "$line" ]]; do
        line=$(echo "$line" | tr -d '[:space:]')
        [[ -z "$line" || "$line" =~ ^# ]] && continue
        [[ "$line" != */* ]] && line="${line}/128"
        echo "allow $line;"
    done < "$TMP_DIR/bunny-v6.txt"
    echo ""
    echo "deny all;"
} > "$BUNNY_TMP"

# Atomic replace
mv -f "$CF_TMP" "$CF_FILE"
mv -f "$BUNNY_TMP" "$BUNNY_FILE"

echo
echo "Done."
echo "  Cloudflare allow: $CF_FILE"
echo "  Bunny.net allow:  $BUNNY_FILE"
