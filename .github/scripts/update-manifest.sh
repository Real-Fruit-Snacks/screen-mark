#!/usr/bin/env bash
# The update manifest of a release: latest.json and its signature.
#
# screen-mark's updater (src/update) fetches latest.json to learn the newest
# version and the size and SHA-256 of each download, and trusts it only if
# latest.json.sig is an Ed25519 signature of it made with one of the keys in
# src/update/keys.rs. The release workflow runs `make`; `keygen` is run once,
# by hand, to create a key.
#
#   update-manifest.sh keygen <file>           new signing key into <file>; prints its public key
#   update-manifest.sh public                  the public key of $UPDATE_SIGNING_KEY
#   update-manifest.sh make <dir> <version> [suffix...]
#                                              write and sign <dir>/latest.json
#   update-manifest.sh verify <dir>            check <dir>/latest.json.sig against src/update/keys.rs
#
# A signing key is 32 random bytes written as 64 hex characters. `make` and
# `public` read it from the UPDATE_SIGNING_KEY environment variable, never
# from the command line. Needs bash, OpenSSL 3 and coreutils.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
trusted_keys="$root/src/update/keys.rs"
# The files a release offers the updater, as the end of their names.
downloads=(windows-x64.zip windows-x64.msi linux-x64.tar.gz)

die() { echo "error: $*" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Bytes on stdin as hex, and back.
hex() { od -An -v -tx1 | tr -d ' \n'; }
unhex() { printf '%b' "$(sed 's/../\\x&/g')"; }

# PEM files OpenSSL can read, from the raw keys. The hex in front is the fixed
# header that says "Ed25519" (RFC 8410).
private_pem() {
  [[ "$1" =~ ^[0-9a-f]{64}$ ]] || die "a signing key is 64 hex characters"
  (
    umask 077
    {
      echo "-----BEGIN PRIVATE KEY-----"
      printf '302e020100300506032b657004220420%s' "$1" | unhex | base64
      echo "-----END PRIVATE KEY-----"
    } > "$work/private.pem"
  )
}
public_pem() {
  {
    echo "-----BEGIN PUBLIC KEY-----"
    printf '302a300506032b6570032100%s' "$1" | unhex | base64
    echo "-----END PUBLIC KEY-----"
  } > "$2"
}
public_of_private() {
  openssl pkey -in "$work/private.pem" -pubout -outform DER | tail -c 32 | hex
}

signing_key() {
  [[ -n "${UPDATE_SIGNING_KEY:-}" ]] || die "set UPDATE_SIGNING_KEY to the signing key"
  private_pem "$(printf '%s' "$UPDATE_SIGNING_KEY" | tr -d ' \r\n')"
}

keygen() {
  local out="${1:-}"
  [[ -n "$out" ]] || die "usage: keygen <file>"
  [[ ! -e "$out" ]] || die "$out already exists; a key is never overwritten"
  local seed
  seed="$(openssl genpkey -algorithm ed25519 -outform DER | tail -c 32 | hex)"
  private_pem "$seed"
  (umask 077; printf '%s\n' "$seed" > "$out")
  echo "Signing key written to $out. Keep it secret."
  echo "Public key, for src/update/keys.rs:"
  public_of_private
  echo
}

verify() {
  local dir="${1:-}"
  [[ -f "$dir/latest.json" && -f "$dir/latest.json.sig" ]] || die "no latest.json and latest.json.sig in '$dir'"
  local n=0 key signature
  while read -r signature || [[ -n "$signature" ]]; do
    [[ "$signature" =~ ^[0-9a-fA-F]{128}$ ]] || continue
    printf '%s' "$signature" | unhex > "$work/signature.bin"
    while read -r key; do
      n=$((n + 1))
      public_pem "$key" "$work/public.pem"
      if openssl pkeyutl -verify -pubin -inkey "$work/public.pem" -rawin \
        -in "$dir/latest.json" -sigfile "$work/signature.bin" < /dev/null > /dev/null 2>&1; then
        echo "latest.json is signed by the trusted key $key"
        return 0
      fi
    done < <(grep -oE '"[0-9a-f]{64}"' "$trusted_keys" | tr -d '"')
  done < "$dir/latest.json.sig"
  [[ $n -gt 0 ]] || die "found no keys in $trusted_keys, or no signature in latest.json.sig"
  die "latest.json.sig was not made with a key in src/update/keys.rs. Is UPDATE_SIGNING_KEY the right key?"
}

make() {
  local dir="${1:-}" version="${2:-}"
  version="${version#v}"
  [[ -d "$dir" ]] || die "usage: make <dir> <version> [suffix...]"
  [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "'$version' is not a version like 2.3.0"
  shift 2
  [[ $# -gt 0 ]] || set -- "${downloads[@]}"
  signing_key

  local suffix name file comma=""
  {
    printf '{\n  "name": "screen-mark",\n  "version": "%s",\n  "files": [' "$version"
    for suffix in "$@"; do
      name="screen-mark-v$version-$suffix"
      file="$dir/$name"
      [[ -f "$file" ]] || die "$file is missing"
      printf '%s\n    { "name": "%s", "size": %s, "sha256": "%s" }' "$comma" "$name" \
        "$(wc -c < "$file" | tr -d ' ')" "$(sha256sum "$file" | cut -c 1-64)"
      comma=","
    done
    printf '\n  ]\n}\n'
  } > "$dir/latest.json"

  openssl pkeyutl -sign -inkey "$work/private.pem" -rawin -in "$dir/latest.json" -out "$work/signature.bin"
  { hex < "$work/signature.bin"; echo; } > "$dir/latest.json.sig"

  cat "$dir/latest.json"
  # A release signed with a key no installed copy trusts would be ignored by
  # every one of them, so that is checked here, before anything is published.
  verify "$dir"
}

case "${1:-}" in
  keygen) shift; keygen "$@" ;;
  public) signing_key; public_of_private; echo ;;
  make) shift; make "$@" ;;
  verify) shift; verify "$@" ;;
  *) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
