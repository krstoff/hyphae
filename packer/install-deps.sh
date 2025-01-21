#!/bin/sudo /bin/bash
set -euo pipefail

dnf install containerd cni-plugins
systemctl enable containerd

mv /tmp/10-containers.network /etc/systemd/network/
mkdir /etc/iproute2
mv /tmp/rt_tables /etc/iproute2/