#!/usr/bin/env bash
# A small Bash example: report disk usage for given directories.

THRESHOLD=80  # percent

dirs=("/" "/tmp" "$HOME")

for dir in "${dirs[@]}"; do
    usage=$(df -h "$dir" 2>/dev/null | awk 'NR==2 {print $5}' | tr -d '%')
    if [ -z "$usage" ]; then
        echo "SKIP  $dir (not mounted)"
        continue
    fi
    if [ "$usage" -ge "$THRESHOLD" ]; then
        echo "WARN  $dir is at ${usage}% (threshold: ${THRESHOLD}%)"
    else
        echo "OK    $dir is at ${usage}%"
    fi
done
