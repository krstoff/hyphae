terraform {
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 4.16"
    }
  }

  required_version = ">= 1.2.0"
}

provider "aws" {
  region  = "us-west-1"
}

resource "aws_vpc" "cluster-vpc" {
  cidr_block = "10.0.0.0/16"
  assign_generated_ipv6_cidr_block = true
  tags = {
    Name = "cluster-vpc"
  }
}

resource "aws_internet_gateway" "igw" {
  vpc_id = aws_vpc.cluster-vpc.id
}

resource "aws_subnet" "node-subnet" {
  vpc_id = aws_vpc.cluster-vpc.id
  cidr_block = "10.0.0.0/20"
  assign_ipv6_address_on_creation = true
  map_public_ip_on_launch = true

  # the important part. this is prefix::0:0/64
  ipv6_cidr_block = cidrsubnet(aws_vpc.cluster-vpc.ipv6_cidr_block, 8, 0)

  tags = {
    Name = "node-subnet"
  }
}

resource "aws_subnet" "container-subnet" {
  vpc_id = aws_vpc.cluster-vpc.id
  cidr_block = "10.0.32.0/20"
  assign_ipv6_address_on_creation = true
  map_public_ip_on_launch = true

  ipv6_cidr_block = cidrsubnet(aws_vpc.cluster-vpc.ipv6_cidr_block, 8, 1)

  tags = {
    Name = "container-subnet"
  }
}

# fresh vpcs don't route to the internet by default
resource "aws_route" "r1" {
  route_table_id = aws_vpc.cluster-vpc.main_route_table_id
  destination_ipv6_cidr_block = "::/0"
  gateway_id = aws_internet_gateway.igw.id
}

resource "aws_route" "r2" {
  route_table_id = aws_vpc.cluster-vpc.main_route_table_id
  destination_cidr_block = "0.0.0.0/0"
  gateway_id = aws_internet_gateway.igw.id
}
