#!/usr/bin/env bash
# 使い方: new-token.sh <name> <token-file>
# トークンを<token-file>に書き、terraform.tfvarsに足す行を表示する。トークン自体は表示しない
# 送信用はclient_token_hashesに、閲覧用はviewer_token_hashesに足す
set -euo pipefail

name=${1:?usage: new-token.sh <client-name> <token-file>}
file=${2:?usage: new-token.sh <client-name> <token-file>}
if [[ ! $name =~ ^[a-z0-9-]+$ ]]; then
  echo "client-name must match [a-z0-9-]+" >&2
  exit 1
fi
if [[ -e $file ]]; then
  echo "$file already exists" >&2
  exit 1
fi

mkdir -p "$(dirname "$file")"
token=$(openssl rand -hex 32)
(umask 077 && printf '%s' "$token" >"$file")
hash=$(printf '%s' "$token" | shasum -a 256 | cut -d' ' -f1)
echo "wrote token to $file"
echo "add to terraform/terraform.tfvars client_token_hashes (sender) or viewer_token_hashes (viewer):"
echo "  \"$name\" = \"$hash\""
