data "aws_caller_identity" "current" {}

locals {
  name = var.project
  # viewer_domainの1つ上をDNSのゾーンとみなす
  dns_zone = join(".", slice(split(".", var.viewer_domain), 1, length(split(".", var.viewer_domain))))
}
