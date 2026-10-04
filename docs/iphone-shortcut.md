# iPhone のショートカットの作り方

he.net Network Tools アプリの traceroute 結果を、共有シートから upstream-save に送るショートカット。
実機ではまだ組んでいないので、アクション名や項目名が iOS のバージョンによって違うことがある。

## 用意するもの

- Function URL（`terraform output -raw function_url`）
- iPhone 用のトークン。`tools/new-token.sh iphone <file>` で作り、ハッシュを `terraform.tfvars` に足して apply しておく。
  トークンはファイルの中身を AirDrop 等で iPhone に渡し、ショートカットに貼る

## 手順

1. ショートカット App で新規ショートカットを作る
2. 詳細設定（ⓘ）で「共有シートに表示」をオンにし、受け入れる入力の種類を「テキスト」だけにする
3. （任意）「入力を要求」アクションを置き、ラベルを聞く。既定値に `tethering` などを入れておく。続けて「URL エンコード」アクションでエンコードする
4. 「URL の内容を取得」アクションを置き、次のように設定する
   - URL: `<Function URL>?format=traceroute-text&label=<3 のエンコード結果>`
   - 方法: `POST`
   - ヘッダ: キー `x-token`、値に iPhone 用のトークン
   - 本文を要求: `ファイル`、値に「ショートカットの入力」
5. 「辞書の値を取得」で応答の `as_path` を取り出し、「結果を表示」で表示する

## 使い方

he.net アプリで traceroute を実行し、結果画面の共有ボタンからこのショートカットを選ぶ。
v4 と v6 は自動で切り替わらないので、両方残したい時は宛先を変えて2回送る。

## 確かめ方

- 401 が返る: トークンが違う。ヘッダ名が `x-token` になっているか確認する
- 422 が返る: 本文を traceroute として読めなかった。応答の `raw_key` に生テキストが残っているので、それを見てパーサを直す
- 送った後は `tools/list-traces.sh` に出る
