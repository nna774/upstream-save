# viewer を S3 オリジンから配る計画

## 目的

`viewer/index.html` と `viewer/viewer.js` は `include_str!` で Lambda に埋め込まれていて、見た目を直すだけでも `cargo lambda build` と Lambda の更新が要る。
フロントエンドを S3 に置き、CloudFront でパスによってオリジンを振り分ければ、フロントエンドの更新は S3 への配置と invalidation だけで済む。

## 構成

| パス | オリジン | キャッシュポリシー |
| --- | --- | --- |
| `/api/*` | Lambda Function URL（今と同じ） | 今の `aws_cloudfront_cache_policy.viewer`（`x-token` をキーに含める） |
| それ以外（default） | S3（OAC 経由） | `Managed-CachingOptimized`（既定 1 日） |

- `default_root_object = "index.html"` で `/` を `index.html` に向ける
- 同じドメインのままなので、`connect-src 'self'` も CORS も変わらない
- POST の取り込みは Function URL に直接送るので影響しない

## CSP 等のヘッダー

S3 は任意の応答ヘッダーを付けられないので、Lambda の `static_response` が付けている `content-security-policy` と `x-content-type-options` を、CloudFront の `aws_cloudfront_response_headers_policy.viewer` の `security_headers_config` に移す。
CSP は記録の label 等（クライアントが自由に入れられる値）による XSS の二段目の防御であり、オリジンを分けても要る。

同じポリシーを両方の behavior に付ける。API の JSON に CSP が付いても害は無い。
`Cache-Control: no-store` の上書きは今と同じく続ける（閲覧者・後段の共有キャッシュには残させない）。

## 変更点

### terraform

新しく `terraform/viewer.tf` を作る:

- `aws_s3_bucket.viewer`（`${local.name}-viewer-${account_id}`）と public access block
- `aws_cloudfront_origin_access_control.viewer`（s3 / always / sigv4）
- `aws_s3_bucket_policy.viewer`: `cloudfront.amazonaws.com` に `s3:GetObject` を、`AWS:SourceArn` をこの distribution に限って許す
- `aws_s3_object.viewer`: `for_each` で `index.html`・`viewer.js` を置く。`source_hash = filemd5(...)`、`content_type` は `text/html; charset=utf-8` / `text/javascript; charset=utf-8` を明示する

`terraform/cloudfront.tf`:

- S3 の `origin`（`bucket_regional_domain_name` + OAC）を足す
- `default_cache_behavior` を S3 に向け、GET/HEAD のみにする
- 今の Lambda 向けの behavior を `ordered_cache_behavior { path_pattern = "/api/*" }` に移す
- response headers policy に CSP と nosniff を足す
- `depends_on = [aws_s3_object.viewer]` で、振り分けを切り替える前に S3 にファイルを置く

### Lambda（`src/main.rs`）

- `VIEWER_HTML`・`VIEWER_JS`・`VIEWER_CSP`・`static_response` と、`/`・`/viewer.js` のルートを消す
- 残る `match` が 1 腕になるので、clippy に合わせて `if` に直す

### README

- 「デプロイ」にフロントエンドだけ出す手順を足す
- 「`/`・`/viewer.js` は CloudFront に1日残る」は S3 でも同じなので、そのまま残す

## デプロイ手順（移行後）

フロントエンドだけ:

```sh
cd terraform
terraform apply -target=aws_s3_object.viewer
aws cloudfront create-invalidation --distribution-id "$(terraform output -raw distribution_id)" --paths '/*'
```

`-target` を付けるのは、ビルドしていない・古い `bootstrap.zip` で Lambda を上書きしないため。

Lambda を含めて全部: 今と同じく `cargo lambda build` → `terraform apply` → invalidation。

## 決めていないこと・懸念

- フロントエンドの配置を terraform（`aws_s3_object` + `-target`）でやるか、`tools/deploy-viewer.sh` で `aws s3 cp` するか。terraform にすると初回の切り替えの順序を `depends_on` で保てるのでそちらを推す
- 初回 apply では、バケットポリシーが distribution の ARN に依存するため、振り分けを切り替えてからポリシーが付くまでの短い間 S3 が 403 を返しうる。個人用なので許容するつもり
- `upstream-save-admin` ロールに、新しいバケット名・OAC・バケットポリシーを作る権限があるか確認が要る
- S3 に無いパスは（ListBucket を許さないので）404 ではなく 403 になる。既存の `custom_error_response` で TTL 0 なので実害は無い
- 移行後は Lambda の CloudFront 経由の応答は `/api/*` だけになる。それ以外のパスを Lambda に直接送っても今と同じく 403 / 404
