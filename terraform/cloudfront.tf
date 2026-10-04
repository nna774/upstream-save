resource "random_password" "origin_secret" {
  length  = 48
  special = false
}

resource "aws_acm_certificate" "viewer" {
  provider          = aws.us_east_1
  domain_name       = var.viewer_domain
  validation_method = "DNS"

  lifecycle {
    create_before_destroy = true
  }
}

# 検証レコードをDNSのrepoに足すまで、applyはここで待つ
resource "aws_acm_certificate_validation" "viewer" {
  provider        = aws.us_east_1
  certificate_arn = aws_acm_certificate.viewer.arn
}

resource "aws_cloudfront_cache_policy" "viewer" {
  name = "${local.name}-viewer"
  # 期間はLambdaのCache-Controlに従う。付いていない応答は残さない
  min_ttl     = 0
  default_ttl = 0
  max_ttl     = 86400

  parameters_in_cache_key_and_forwarded_to_origin {
    enable_accept_encoding_gzip   = true
    enable_accept_encoding_brotli = true

    # トークン付きのリクエストに、トークン無しで残した応答を返さないため。トークン付きの応答はno-storeなので残らない
    headers_config {
      header_behavior = "whitelist"
      headers {
        items = ["x-token"]
      }
    }

    query_strings_config {
      query_string_behavior = "none"
    }

    cookies_config {
      cookie_behavior = "none"
    }
  }
}

# LambdaのCache-ControlはCloudFrontのキャッシュのためのもの。後段の共有キャッシュはinvalidationで消せないので、閲覧者には残させない
resource "aws_cloudfront_response_headers_policy" "viewer" {
  name = "${local.name}-viewer"

  custom_headers_config {
    items {
      header   = "Cache-Control"
      value    = "no-store"
      override = true
    }
  }
}

resource "aws_cloudfront_distribution" "viewer" {
  enabled         = true
  is_ipv6_enabled = true
  http_version    = "http2and3"
  price_class     = "PriceClass_200"
  aliases         = [var.viewer_domain]

  origin {
    origin_id   = "lambda"
    domain_name = trimsuffix(trimprefix(aws_lambda_function_url.api.function_url, "https://"), "/")

    custom_origin_config {
      http_port              = 80
      https_port             = 443
      origin_protocol_policy = "https-only"
      origin_ssl_protocols   = ["TLSv1.2"]
    }

    custom_header {
      name  = "x-origin-verify"
      value = random_password.origin_secret.result
    }
  }

  default_cache_behavior {
    target_origin_id = "lambda"
    # 公開の切り替えにPUTとDELETEを通す
    allowed_methods            = ["GET", "HEAD", "OPTIONS", "PUT", "POST", "PATCH", "DELETE"]
    cached_methods             = ["GET", "HEAD"]
    viewer_protocol_policy     = "redirect-to-https"
    compress                   = true
    cache_policy_id            = aws_cloudfront_cache_policy.viewer.id
    response_headers_policy_id = aws_cloudfront_response_headers_policy.viewer.id
  }

  # Lambdaのエラーにno-storeが付かないことがあり、付いていても尊重されないコードがある
  dynamic "custom_error_response" {
    for_each = [400, 403, 404, 405, 414, 416, 500, 501, 502, 503, 504]
    content {
      error_code            = custom_error_response.value
      error_caching_min_ttl = 0
    }
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    acm_certificate_arn      = aws_acm_certificate_validation.viewer.certificate_arn
    ssl_support_method       = "sni-only"
    minimum_protocol_version = "TLSv1.2_2021"
  }
}
