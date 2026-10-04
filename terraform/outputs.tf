output "function_url" {
  value = aws_lambda_function_url.api.function_url
}

output "bucket" {
  value = aws_s3_bucket.data.bucket
}
