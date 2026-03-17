#!/usr/bin/env bash
set -euo pipefail

CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
CHROME_DATA="/tmp/scavenger-chrome"
CDP_PORT=9222
SOCKET="${HOME}/.run/scavenger/daemon.sock"
DAEMON_LOG="${HOME}/.local/share/scavenger/daemon.log"
DB="${HOME}/.local/share/scavenger/scavenger.db"

red()   { printf '\033[31m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
dim()   { printf '\033[2m%s\033[0m\n' "$*"; }
bold()  { printf '\033[1m%s\033[0m\n' "$*"; }

chrome_pid() { lsof -ti "tcp:${CDP_PORT}" -sTCP:LISTEN 2>/dev/null || true; }
daemon_pid() { pgrep -f 'scavenger-ctl start' 2>/dev/null | head -1 || true; }

daemon_alive() {
    [ -S "${SOCKET}" ] || return 1
    echo '{"command":"status"}' | nc -U "${SOCKET}" 2>/dev/null | grep -q '"ok"'
}

# Start Chrome in headless mode (default) or visible mode for login
# Usage: start_chrome [--visible]
start_chrome() {
    local mode="headless"
    [ "${1:-}" = "--visible" ] && mode="visible"

    if [ -n "$(chrome_pid)" ]; then
        dim "Chrome CDP :${CDP_PORT} already up"
        return
    fi

    mkdir -p "${CHROME_DATA}"
    local flags=(
        --remote-debugging-port="${CDP_PORT}"
        --user-data-dir="${CHROME_DATA}"
        --no-first-run
        --disable-default-apps
    )
    if [ "$mode" = "headless" ]; then
        bold "Starting Chrome (headless)..."
        flags+=(--headless=new)
    else
        bold "Starting Chrome..."
    fi

    "${CHROME}" "${flags[@]}" >/dev/null 2>&1 &
    local i=0
    while [ -z "$(chrome_pid)" ] && [ "$i" -lt 20 ]; do sleep 0.25; i=$((i+1)); done
    if [ -n "$(chrome_pid)" ]; then
        green "Chrome ready  pid=$(chrome_pid)"
    else
        red "Chrome failed to start"; exit 1
    fi
}

ensure_chrome() { start_chrome; }

ensure_daemon() {
    if daemon_alive; then
        dim "Daemon already up"
        return
    fi

    # Kill anything leftover that lost its socket
    pkill -f 'scavenger-ctl start' 2>/dev/null || true
    rm -f "${SOCKET}"
    sleep 0.5

    uv sync --all-extras --quiet 2>&1 | grep -v '^$' || true

    bold "Starting daemon..."
    mkdir -p "$(dirname "${DAEMON_LOG}")" "$(dirname "${SOCKET}")"
    nohup uv run scavenger-ctl start >>"${DAEMON_LOG}" 2>&1 &
    disown

    local i=0
    while [ "$i" -lt 30 ]; do
        if daemon_alive; then
            green "Daemon ready  pid=$(daemon_pid)  log=${DAEMON_LOG}"
            return
        fi
        sleep 1; i=$((i+1))
        # Progress dot every 5s so it doesn't look hung
        [ $((i % 5)) -eq 0 ] && printf '.'
    done
    echo
    red "Daemon not responding after 30s — last log lines:"
    tail -5 "${DAEMON_LOG}" 2>/dev/null | while IFS= read -r l; do red "  $l"; done
    exit 1
}

cmd_start() {
    ensure_chrome
    ensure_logins
    ensure_daemon
    echo
    cmd_status
}

cmd_stop() {
    if daemon_alive || [ -n "$(daemon_pid)" ]; then
        bold "Stopping daemon..."
        # Graceful shutdown via socket first
        echo '{"command":"shutdown"}' | nc -U "${SOCKET}" >/dev/null 2>&1 || true
        sleep 1
        # Kill any remaining processes
        pkill -f 'scavenger-ctl start' 2>/dev/null || true
        sleep 0.5
        pkill -9 -f 'scavenger-ctl start' 2>/dev/null || true
        rm -f "${SOCKET}"
        green "Daemon stopped"
    else
        dim "Daemon not running"
    fi

    local cpid; cpid=$(chrome_pid)
    if [ -n "$cpid" ]; then
        bold "Stopping Chrome (pid ${cpid})..."
        kill "$cpid" 2>/dev/null || true
        green "Chrome stopped"
    else
        dim "Chrome not running"
    fi
}

cmd_status() {
    bold "scavenger status"
    echo
    local cpid; cpid=$(chrome_pid)
    [ -n "$cpid" ] && green "  chrome   up  pid=${cpid} CDP=:${CDP_PORT}" || red "  chrome   down"

    if daemon_alive; then
        local dpid; dpid=$(daemon_pid)
        green "  daemon   up  pid=${dpid:-?}"
    else
        red "  daemon   down"
    fi

    [ -S "${SOCKET}" ] && dim "  socket   ${SOCKET}"
    if [ -f "$DB" ]; then
        local sz; sz=$(du -h "$DB" | cut -f1)
        local ct; ct=$(sqlite3 "$DB" "SELECT count(*) FROM listings;" 2>/dev/null || echo "?")
        dim "  db       ${sz}  ${ct} listings"
    fi
}

cmd_log() {
    [ -f "${DAEMON_LOG}" ] && exec tail -f "${DAEMON_LOG}"
    red "No log at ${DAEMON_LOG}"; exit 1
}

cmd_tui() {
    daemon_alive || { dim "Daemon not running, starting..."; cmd_start; echo; }
    exec uv run scavenger "$@"
}

# Check if a facebook tab is on a logged-in page (not /login)
fb_tab_logged_in() {
    curl -s "http://localhost:${CDP_PORT}/json/list" 2>/dev/null | python3 -c "
import sys, json
try:
    tabs = json.load(sys.stdin)
except: sys.exit(1)
for t in tabs:
    url = t.get('url', '')
    if 'facebook.com' in url and '/login' not in url and '/recover' not in url:
        sys.exit(0)
sys.exit(1)
" 2>/dev/null
}

# Open facebook and wait for the user to log in, polling until they do
ensure_fb_login() {
    # Quick check with headless Chrome — open facebook, see if it redirects to login
    curl -s -X PUT "http://localhost:${CDP_PORT}/json/new?https://www.facebook.com/" >/dev/null
    sleep 3

    if fb_tab_logged_in; then
        green "Facebook session active"
        return
    fi

    # Need visible Chrome for login — stop headless, start visible
    local cpid; cpid=$(chrome_pid)
    [ -n "$cpid" ] && kill "$cpid" 2>/dev/null && sleep 1

    start_chrome --visible
    curl -s -X PUT "http://localhost:${CDP_PORT}/json/new?https://www.facebook.com/login" >/dev/null

    echo
    bold "Facebook login required."
    dim "Log in to Facebook in the Chrome window."
    dim "Waiting..."
    echo

    while ! fb_tab_logged_in; do
        sleep 2
    done
    green "Facebook login verified"

    # Switch back to headless for scraping
    sleep 1
    cpid=$(chrome_pid)
    [ -n "$cpid" ] && kill "$cpid" 2>/dev/null && sleep 1
    start_chrome
}

# Check which sources need logins and handle them
ensure_logins() {
    local config="${HOME}/.config/scavenger/config.toml"
    [ -f "$config" ] || return

    if grep -q '"facebook"' "$config" 2>/dev/null; then
        ensure_fb_login
    fi
}

case "${1:-}" in
    start)   cmd_start ;;
    stop)    cmd_stop ;;
    status)  cmd_status ;;
    log)     cmd_log ;;
    login)   start_chrome --visible; ensure_logins ;;
    tui)     shift; cmd_tui "$@" ;;
    restart) cmd_stop; echo; cmd_start ;;
    *)       echo "Usage: $0 {start|stop|status|log|login|tui|restart}" ;;
esac
