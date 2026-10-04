variable "region" {
  type    = string
  default = "ap-northeast-1"
}

variable "project" {
  type    = string
  default = "upstream-save"
}

variable "client_token_hashes" {
  description = "クライアント名からトークンのSHA-256 hexへの対応。tools/new-token.shで生成する"
  type        = map(string)

  validation {
    condition     = alltrue([for name in keys(var.client_token_hashes) : can(regex("^[a-z0-9-]+$", name))])
    error_message = "クライアント名は[a-z0-9-]だけで書く。S3キーに入るため"
  }

  validation {
    condition     = alltrue([for h in values(var.client_token_hashes) : can(regex("^[0-9a-f]{64}$", h))])
    error_message = "値はSHA-256の小文字hex(64桁)で書く"
  }

  validation {
    condition     = !contains(values(var.client_token_hashes), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
    error_message = "空文字列のSHA-256は登録できない"
  }

  validation {
    condition     = length(distinct(values(var.client_token_hashes))) == length(var.client_token_hashes)
    error_message = "同じハッシュを複数のクライアント名に登録できない"
  }
}
