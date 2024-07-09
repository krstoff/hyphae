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
}

resource "aws_network_interface" "container-eni" {
  count = var.node_count
  subnet_id   = aws_subnet.container-subnet.id
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
  depends_on = [ aws_network_interface.container-eni ]
}
