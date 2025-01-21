variable "node_count" {
  type = number
  default = 0
}

variable "commit_id" {}

output instances {
  value = {
    for index, node in aws_instance.node:
      node.id => node.ipv6_addresses[0]
  }
}

data "aws_ami" "hyphae-node-image" {
  filter {
    name   = "name"
    values = ["hyphae-node-${var.commit_id}"]
  }
  owners = ["055838255245"]
}

resource "aws_network_interface" "node-eni" {
  count = var.node_count
  subnet_id = aws_subnet.node-subnet.id
  ipv6_address_count = 1
}

resource "aws_network_interface" "container-eni" {
  count = var.node_count
  subnet_id   = aws_subnet.node-subnet.id
  ipv6_prefix_count = 1
  source_dest_check = false
}

resource "aws_instance" "node" {
  count = var.node_count
  ami           = data.aws_ami.hyphae-node-image.id
  instance_type = "t3.micro"
  key_name = "skeleton-key"
  
  network_interface {
    network_interface_id = aws_network_interface.node-eni[count.index].id
    device_index = 0
  }
  network_interface {
    network_interface_id = aws_network_interface.container-eni[count.index].id
    device_index = 1
  }

  user_data = <<-EOF
  #cloud-config
  bootcmd:
    - ip -6 rule add from ${one(aws_network_interface.container-eni[count.index].ipv6_prefixes)} lookup container_routing
    - ip -6 route add ${one(aws_network_interface.node-eni[count.index].ipv6_addresses)} dev ens5 table container_routing

  write_files:
    - content: |
        {
          "name": "container-network",
          "type": "ipvlan",
          "master": "ens6",
          "mode": "l3",
          "ipam": {
            "type": "host-local",
            "routes": [
              { "dst": "::/0" }
            ],
            "subnet": "${one(aws_network_interface.container-eni[count.index].ipv6_prefixes)}"
          }
        }
      path: "/etc/cni/net.d/10-ipvlan.conf"
  EOF
}
