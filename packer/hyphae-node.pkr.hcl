packer {
  required_plugins {
    amazon = {
      version = ">= 1.2.8"
      source  = "github.com/hashicorp/amazon"
    }
  }
}

variable "commit-id" {}

source "amazon-ebs" "al2" {
  ami_name      = "hyphae-node-${var.commit-id}"
  instance_type = "t3.micro"
  region        = "us-west-1"
  source_ami_filter {
    filters = {
      name                = "al2023-ami-2023.6.20250115.0-kernel-6.1-x86_64"
      root-device-type    = "ebs"
      virtualization-type = "hvm"
    }
    most_recent = true
    owners      = ["137112412989"]
  }
  ssh_username = "ec2-user"
}

build {
  name    = "hyphae-node"
  sources = [
    "source.amazon-ebs.al2"
  ]

  provisioner "shell" {
    script = "install-deps.sh"
  }
}
