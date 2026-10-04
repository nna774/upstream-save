#!/usr/bin/env bash
# 使い方: trace.sh <target> [label]
# IPv4とIPv6の経路をmtrで並列に測り、ファミリごとに別の記録として送る
set -euo pipefail

conf=${XDG_CONFIG_HOME:-$HOME/.config}/upstream-save
target=${1:?usage: trace.sh <target> [label]}
label=${2:-}
url=$(<"$conf/url")
token=$(<"$conf/token")
mtr=$(command -v mtr || echo /opt/homebrew/sbin/mtr)

uri() { jq -rn --arg v "$1" '$v|@uri'; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# バックグラウンドのsudoはパスワードを聞けないので、先に認証を済ませる
sudo -v
for af in 4 6; do
  sudo "$mtr" "-$af" -b -c 10 --json "$target" >"$tmp/$af.json" 2>"$tmp/$af.err" &
  echo $! >"$tmp/$af.pid"
done

sent=0
failed=0
for af in 4 6; do
  if ! wait "$(<"$tmp/$af.pid")" || ! jq -e '.report.hubs | length > 0' "$tmp/$af.json" >/dev/null 2>&1; then
    echo "IPv$af: skipped: $(tr '\n' ' ' <"$tmp/$af.err")" >&2
    continue
  fi
  query="format=mtr-json&af=$af&target=$(uri "$target")"
  [[ -n $label ]] && query+="&label=$(uri "$label")"
  post() {
    curl -sS --fail-with-body --connect-timeout 10 --max-time 60 "$@" \
      -X POST -H "x-token: $token" -H 'content-type: application/json' \
      --data-binary "@$tmp/$af.json" -w '\n' "${url%/}/?$query"
  }
  # source_ipが測定と同じファミリになるよう揃える。そのファミリでLambdaに届かない時だけ指定を外す
  rc=0
  post "-$af" || rc=$?
  if [[ $rc == 6 || $rc == 7 ]]; then
    echo "IPv$af: cannot reach the endpoint over IPv$af, retrying without family" >&2
    rc=0
    post || rc=$?
  fi
  if [[ $rc == 0 ]]; then
    sent=$((sent + 1))
  else
    echo "IPv$af: POST failed (curl exit $rc)" >&2
    failed=$((failed + 1))
  fi
done

if [[ $sent == 0 || $failed != 0 ]]; then
  exit 1
fi
