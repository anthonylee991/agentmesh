#!/bin/bash
# AgentMesh inbox watcher - runs in background, exits when a message arrives
# Claude Code gets notified when this script exits, triggering action
# Usage: bash watch_inbox.sh <agent_id> [max_wait_secs]

AGENT_ID="$1"
if [ -z "$AGENT_ID" ]; then
    echo "[AgentMesh watcher] ERROR: agent_id argument required. Usage: bash watch_inbox.sh <agent_id> [max_wait_secs]"
    exit 1
fi

INBOX_FILE="$HOME/.agentmesh/inbox_${AGENT_ID}.json"
POLL_INTERVAL=2  # seconds between checks
MAX_WAIT="${2:-7200}"  # default: 2 hours, overridable via second argument

# Find python (macOS may only have python3)
PYTHON="python"
if ! command -v python &>/dev/null; then
    PYTHON="python3"
fi

elapsed=0

while [ $elapsed -lt $MAX_WAIT ]; do
    if [ -f "$INBOX_FILE" ]; then
        COUNT=$($PYTHON -c "import json,sys; d=json.load(open(sys.argv[1])); print(d.get('count',0))" "$INBOX_FILE" 2>/dev/null)

        if [ -n "$COUNT" ] && [ "$COUNT" != "0" ]; then
            MESSAGES=$($PYTHON -c "
import json, sys
d = json.load(open(sys.argv[1]))
for m in d.get('messages', []):
    mtype = m.get('type', 'Unknown')
    frm = m.get('from', 'unknown')
    preview = m.get('preview', '')
    print(f'  [{mtype}] from {frm}: {preview}')
" "$INBOX_FILE" 2>/dev/null)

            echo ""
            echo "=== AGENTMESH: $COUNT new message(s) received! ==="
            echo "$MESSAGES"
            echo ""
            echo "ACTION REQUIRED: Call mesh_check_messages to view and respond, then restart the watcher with: bash ~/.agentmesh/watch_inbox.sh $AGENT_ID $MAX_WAIT"
            echo "=============================================="
            exit 0
        fi
    fi

    sleep $POLL_INTERVAL
    elapsed=$((elapsed + POLL_INTERVAL))
done

echo "[AgentMesh watcher] No messages after ${MAX_WAIT}s. Restart watcher with: bash ~/.agentmesh/watch_inbox.sh $AGENT_ID $MAX_WAIT"
exit 0
