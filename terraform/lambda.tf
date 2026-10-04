locals {
  # `cargo lambda build --release --arm64 --output-format zip`の出力。apply前にビルドしておく
  lambda_zip = "${path.module}/../target/lambda/upstream-save/bootstrap.zip"
}

resource "aws_lambda_function" "api" {
  function_name    = local.name
  role             = aws_iam_role.lambda.arn
  runtime          = "provided.al2023"
  architectures    = ["arm64"]
  handler          = "bootstrap"
  filename         = local.lambda_zip
  source_code_hash = try(filebase64sha256(local.lambda_zip), null)
  # RIPEstatへの問い合わせを含むので、既定の3秒では足りない
  timeout     = 30
  memory_size = 128
  # Function URLは認証NONEで誰でも起動できるので、アカウント共有の同時実行枠を食い潰されないよう絞る
  reserved_concurrent_executions = 2

  environment {
    variables = {
      BUCKET       = aws_s3_bucket.data.bucket
      TOKEN_HASHES = jsonencode(var.client_token_hashes)
    }
  }
}

resource "aws_lambda_function_url" "api" {
  function_name = aws_lambda_function.api.function_name
  # x-tokenヘッダで認証する。provider 6.28以降は、NONEの時に要るlambda:InvokeFunctionの許可も自動で足す
  authorization_type = "NONE"
}
