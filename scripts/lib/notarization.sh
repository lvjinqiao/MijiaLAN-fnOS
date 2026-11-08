#!/usr/bin/env bash

# Apple may accept a submission before the ticket endpoint is reachable.
# Keep a permanent failure visible after bounded retries.
retry_notarization_ticket() {
  local retry_attempt=1 retry_delay=5 command_status
  while true; do
    if "$@"; then
      return 0
    else
      command_status=$?
    fi
    if [[ "$retry_attempt" -ge 6 ]]; then
      return "$command_status"
    fi
    echo "Notarization ticket command failed (attempt $retry_attempt/6); retrying in ${retry_delay}s" >&2
    sleep "$retry_delay"
    retry_attempt=$((retry_attempt + 1))
    retry_delay=$((retry_delay * 2))
    if [[ "$retry_delay" -gt 30 ]]; then
      retry_delay=30
    fi
  done
}
