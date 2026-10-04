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
