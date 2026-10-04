output "function_url" {
  value = aws_lambda_function_url.api.function_url
}

output "bucket" {
  value = aws_s3_bucket.data.bucket
}

output "viewer_url" {
  value = "https://${var.viewer_domain}/"
}

output "distribution_id" {
  value = aws_cloudfront_distribution.viewer.id
}

# DNSのrepoで`var.viewer_domain`のCNAMEに向ける先
output "cloudfront_domain" {
  value = aws_cloudfront_distribution.viewer.domain_name
}

# DNSのrepoに足すACMの検証レコード（CNAME）
output "acm_validation_records" {
  value = {
    for o in aws_acm_certificate.viewer.domain_validation_options : o.resource_record_name => o.resource_record_value
  }
}
