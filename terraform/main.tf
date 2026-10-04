data "aws_caller_identity" "current" {}

locals {
  name = var.project
}
