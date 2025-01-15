packer {
  required_plugins {
    amazon = {
      version = ">= 1.2.8"
      source  = "github.com/hashicorp/amazon"
    }
  }
}

variable "commit-id" {}

source "amazon-ebs" "alpine" {
  ami_name      = "hyphae-node-${var.commit-id}"
  instance_type = "t3.micro"
  region        = "us-west-1"
  source_ami_filter {
    filters = {
      name                = "alpine-3.20.*.-x86_64-uefi-cloudinit-r0"
      root-device-type    = "ebs"
      virtualization-type = "hvm"
    }
    most_recent = true
    owners      = ["538276064493"]
  }
  ssh_username = "alpine"
}

build {
  name    = "hyphae-node"
  sources = [
    "source.amazon-ebs.alpine"
  ]

  provisioner "shell" {
    script = "install-deps.sh"
  }
}
