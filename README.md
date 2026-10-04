# upstream-save

テザリング等の普段使わない回線から、どの経路（hop の IP と AS）で外に出ているかを記録する。
クライアントは mtr / traceroute の出力を POST するだけで、パース・AS 補完（RIPEstat）・S3 への保存は Lambda が行う。

## 記録の取り方

### Mac

```sh
tools/trace.sh <target> [label]
```

IPv4 と IPv6 の経路を `sudo mtr -b -c 10 --json` で並列に測り、ファミリごとに1件ずつ送る。
そのファミリの経路が無い時は、送らずに飛ばす。

事前に `~/.config/upstream-save/url`（Function URL）と `~/.config/upstream-save/token` を置いておく。

### iPhone

he.net Network Tools アプリの traceroute 結果を、共有シートからショートカットで送る。作り方は [docs/iphone-shortcut.md](docs/iphone-shortcut.md)。

### 手で送る

```sh
curl -X POST -H "x-token: $(cat ~/.config/upstream-save/token)" \
  "$URL?format=traceroute-text&label=tethering" --data-binary @trace.txt
```

| クエリ | 意味 |
| --- | --- |
| `format` | `mtr-json` / `mtr-text` / `traceroute-text`。省略すると本文から推定する |
| `af` | `4` / `6`。省略すると最初に応答した hop の IP から決める |
| `label` | 任意のラベル |
| `target` | 宛先。省略すると本文から拾う |

## 保存されるもの

- `traces/YYYY-MM/<YYYYMMDDTHHMMSSZ>-<client>-v<af>.json`: パースと補完をした結果
- `raw/YYYY-MM/<同>.txt`: 受け取った本文そのまま。パースに失敗した時もこちらは残る

`<client>` はトークンに対応する名前。

一覧を TSV で見る:

```sh
tools/list-traces.sh
```

## トークンの追加

```sh
tools/new-token.sh <client-name> ~/.config/upstream-save/token
```

表示された行を `terraform/terraform.tfvars` の `client_token_hashes` に足して apply する。
Lambda にはトークンの SHA-256 だけを渡す。

## 開発

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

## デプロイ

`cargo-lambda`（`brew install cargo-lambda/tap/cargo-lambda`）と Terraform が要る。
AWS は `upstream-save-admin` ロールを使う（`.envrc` で `AWS_PROFILE=upstream-save-admin`）。

```sh
cargo lambda build --release --arm64 --output-format zip
cd terraform
terraform init
terraform apply
```
