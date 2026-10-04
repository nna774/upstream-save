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

# DNSのrepoのrecords.ymlはゾーンからの相対名で書き、末尾の`.`を付けない
output "acm_validation_records" {
  value = {
    for o in aws_acm_certificate.viewer.domain_validation_options :
    trimsuffix(o.resource_record_name, ".${local.dns_zone}.") => trimsuffix(o.resource_record_value, ".")
  }
}
