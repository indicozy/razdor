#!/bin/sh
# Alerts on a burst of errors in the API's own nginx log (razdor_api format, see nginx.conf).
# Looks at the last 10 minutes: 5xx responses, and 4xx responses except 404 and 429. It alerts when
# there are at least 5 server errors, or at least 20 other client errors. It logs the alert to the
# journal (user.crit, tag razdor-api-alert) and appends the same line, in UTC, to /root/API-ALERT.
# It always exits 0: a broken check must not look like a failed service.
# Run by razdor-api-alert.timer every 10 minutes. Install: see server/deploy/README.md, section 7.

LOG=${RAZDOR_API_LOG:-/var/log/nginx/razdor-api.log}
ALERT_FILE=${RAZDOR_API_ALERT_FILE:-/root/API-ALERT}
WINDOW_MIN=10
SERVER_ERRORS_LIMIT=5
CLIENT_ERRORS_LIMIT=20

[ -r "$LOG" ] || exit 0

# The log time is the host's local ISO time, so the cutoff uses the same local format.
cutoff=$(date -d "-$WINDOW_MIN min" +%Y-%m-%dT%H:%M:%S 2>/dev/null) || exit 0
[ -n "$cutoff" ] || exit 0

# One line: "<5xx count> <other 4xx count>".
tally=$(awk -v c="$cutoff" '
    substr($1, 1, 19) >= c {
        s = $4
        if (s ~ /^5/) five++
        else if (s ~ /^4/ && s != 404 && s != 429) four++
    }
    END { printf "%d %d\n", five + 0, four + 0 }
' "$LOG" 2>/dev/null) || exit 0
five=${tally% *}
four=${tally#* }

if [ "$five" -ge "$SERVER_ERRORS_LIMIT" ] || [ "$four" -ge "$CLIENT_ERRORS_LIMIT" ]; then
    top=$(awk -v c="$cutoff" '
        substr($1, 1, 19) >= c {
            s = $4
            if (s ~ /^5/ || (s ~ /^4/ && s != 404 && s != 429)) print $2, $3, s
        }
    ' "$LOG" 2>/dev/null | sort | uniq -c | sort -rn | head -n 3 | awk '
        {
            n = $1
            $1 = ""
            sub(/^ /, "")
            printf "%s%s x%d", (NR > 1 ? "; " : ""), $0, n
        }
    ')
    msg="${five} server errors (5xx) and ${four} client errors (4xx, not 404 or 429) in ${WINDOW_MIN} min; top: ${top}"
    if command -v logger >/dev/null 2>&1; then
        logger -p user.crit -t razdor-api-alert "$msg" 2>/dev/null || true
    fi
    printf '%s %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$msg" >> "$ALERT_FILE" 2>/dev/null || true
fi
exit 0
