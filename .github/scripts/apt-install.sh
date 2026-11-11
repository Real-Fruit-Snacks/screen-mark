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
codename=$(. /etc/os-release && echo "$VERSION_CODENAME")
if [[ -f "$list" ]]; then
  # apt tries every mirror, with retries, for every file before it reaches
  # the next one, so a dead mirror costs minutes. Keep the ones that answer
  # now, in their order, then the fallback.
  answering=()
  while IFS=$'\t' read -r mirror _; do
    [[ -n "$mirror" && "$mirror" != "$fallback" ]] || continue
    if curl -fsS -o /dev/null --max-time 10 "${mirror%/}/dists/$codename/InRelease"; then
      answering+=("$mirror")
    else
      echo "::warning::the package mirror $mirror isn't answering; skipping it"
    fi
  done < "$list"
  answering+=("$fallback")
  for i in "${!answering[@]}"; do
    printf '%s\tpriority:%s\n' "${answering[$i]}" "$(( i + 1 ))"
  done | sudo tee "$list" >/dev/null
  echo "Package mirrors, in order:"
  cat "$list"
fi

# The loop below retries; apt itself gives up on a silent server quickly.
opts=(-o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 -o Acquire::Retries=1)
for attempt in 1 2 3; do
  if sudo apt-get "${opts[@]}" update && sudo apt-get "${opts[@]}" install -y "$@"; then
    exit 0
  fi
  echo "::warning::installing packages failed (attempt $attempt of 3)"
  sleep $(( attempt * 15 ))
done
echo "::error::couldn't install packages: every Ubuntu mirror failed" >&2
exit 1
