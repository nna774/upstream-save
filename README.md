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
| `target` | 宛先。省略すると本文から拾う（mtr のテキストからは拾えない） |

## 保存されるもの

- `traces/YYYY-MM/<YYYYMMDDTHHMMSSZ>-<client>-v<af>.json`: パースと補完をした結果
- `raw/YYYY-MM/<同>.txt`: 受け取った本文そのまま。パースに失敗した時もこちらは残る

`<client>` はトークンに対応する名前。ファミリが分からない時（`af` が無く、応答した hop も無い時）は `-v<af>` が付かない。
同じキーが既にあれば上書きせず 409 を返す。本文は 256KB まで。

`as_path` は hop 順に AS を並べ、連続する同じ AS を畳んだもの。同じ hop に複数の応答元がある時は、AS が分かる最初の応答元だけを使う。
`lookup_failed` には、RIPEstat への問い合わせがエラーか期限切れで補完できなかった IP が入る。経路広告されていない IP は入らない。

一覧を TSV で見る:

```sh
tools/list-traces.sh
```

## ビューワ

https://upstream-save.dark-kuins.net/ を開くと、記録の一覧・hop の詳細・label と af ごとの AS path の比較が見られる。
前段の CloudFront を通さない閲覧は Lambda が 403 で拒む。Function URL は記録を送る POST 専用である。

- 画面右上に閲覧用トークンを貼ると、すべての記録が見える。トークンはブラウザの localStorage に残る
- 詳細画面の「公開する」で、その記録をトークン無しでも見られるようにする。公開した記録は source_ip や client 名も含めてそのまま見える
- トークン無しで開くと、公開した記録だけが見える

公開状態は `public/YYYY-MM/<同>` の空のオブジェクトで持つ。

トークン無しの応答と `/`・`/viewer.js` は CloudFront に1日残る（ブラウザには残らない）。
記録を足したり公開したりしても、トークン無しの人には最大1日見えない。トークン付きの API 応答はキャッシュされない。
デプロイした時と、公開をやめた時は invalidation する:

```sh
cd terraform
aws cloudfront create-invalidation --distribution-id "$(terraform output -raw distribution_id)" --paths '/*'
```

閲覧の API:

| メソッド・パス | 内容 |
| --- | --- |
| `GET /api/traces` | 要約の一覧。トークン無しなら公開分だけ |
| `GET /api/traces/YYYY-MM/<id>` | 記録1件 |
| `PUT` / `DELETE /api/traces/YYYY-MM/<id>/public` | 公開する / やめる。閲覧用トークンが要る |

閲覧用トークンでは記録を送れず、送信用トークンでは非公開の記録を読めない。

## トークンの追加

```sh
tools/new-token.sh <client-name> ~/.config/upstream-save/token
```

表示された行を `terraform/terraform.tfvars` の `client_token_hashes` に足して apply する。
閲覧用は、別のファイルに作って `viewer_token_hashes` に足す。
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

ビューワの DNS は [dark-kuins.net-dns](https://github.com/nna774/dark-kuins.net-dns) の `records.yml` で管理している。
初めて作る時は、ACM の検証が済むまで CloudFront を作れないので、次の順に進める。

1. `terraform apply -target=aws_acm_certificate.viewer` の後、`terraform output acm_validation_records` を `dark-kuins.net:` の `acm:` に足す
2. `terraform apply`。検証レコードが引けるまで待つ
3. `terraform output cloudfront_domain` を `dark-kuins.net:` の `cname:` に `upstream-save` として足す

## ライセンス

MIT
