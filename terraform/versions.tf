terraform {
  required_version = ">= 1.10"

  backend "s3" {
    bucket       = "nana-terraform-state"
    key          = "upstream-save.tfstate"
    region       = "ap-northeast-1"
    use_lockfile = true
    encrypt      = true
  }

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.28"
    }
    random = {
      source  = "hashicorp/random"
      version = "~> 3.7"
    }
  }
}

provider "aws" {
  region = var.region
  default_tags {
    tags = {
      Project = var.project
    }
  }
}

# CloudFrontに付ける証明書はus-east-1に置く必要がある
provider "aws" {
  alias  = "us_east_1"
  region = "us-east-1"
  default_tags {
    tags = {
      Project = var.project
    }
  }
}
