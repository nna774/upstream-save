#!/usr/bin/env bash
# 保存済みの記録を手元の.cache/tracesに同期し、ts・client・label・af・target・as_pathのTSVで出す
set -euo pipefail

cd "$(dirname "$0")/.."
bucket=${UPSTREAM_SAVE_BUCKET:-$(terraform -chdir=terraform output -raw bucket)}
aws s3 sync "s3://$bucket/traces/" .cache/traces/ --only-show-errors

printf 'ts\tclient\tlabel\taf\ttarget\tas_path\n'
find .cache/traces -name '*.json' -print0 | sort -z |
  xargs -0 jq -r '[.ts, .client, .label // "", (.af // "" | tostring), .target // "", (.as_path | join(" "))] | @tsv'
