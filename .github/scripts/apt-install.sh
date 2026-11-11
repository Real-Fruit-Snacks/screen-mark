#!/usr/bin/env bash
# Install Ubuntu packages on a CI runner, surviving a mirror that is down.
#
#   .github/scripts/apt-install.sh <package>...
#
# GitHub's runners fetch packages through a mirror list
# (/etc/apt/apt-mirrors.txt: Azure's mirror, then Canonical's). When all of
# those stop answering, apt waits two minutes on each and the job hangs.
# This adds kernel.org's mirror as the last choice and makes apt give up on
# a silent server after 20 seconds. Packages are checked against Ubuntu's
# signing keys whichever mirror they come from.
set -euo pipefail

[[ $# -gt 0 ]] || { echo "usage: apt-install.sh <package>..." >&2; exit 2; }

list=/etc/apt/apt-mirrors.txt
fallback="http://mirrors.edge.kernel.org/ubuntu/"
if [[ -f "$list" ]] && ! grep -q "$fallback" "$list"; then
  last=$(grep -o 'priority:[0-9]*' "$list" | cut -d: -f2 | sort -n | tail -n 1)
  printf '%s\tpriority:%s\n' "$fallback" "$(( ${last:-0} + 1 ))" | sudo tee -a "$list" >/dev/null
fi
[[ -f "$list" ]] && { echo "Package mirrors, in order:"; cat "$list"; }

opts=(-o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 -o Acquire::Retries=3)
for attempt in 1 2 3; do
  if sudo apt-get "${opts[@]}" update && sudo apt-get "${opts[@]}" install -y "$@"; then
    exit 0
  fi
  echo "::warning::installing packages failed (attempt $attempt of 3)"
  sleep $(( attempt * 15 ))
done
echo "::error::couldn't install packages: every Ubuntu mirror failed" >&2
exit 1
