#!/bin/sudo /bin/bash
set -euo pipefail

dnf -y install containerd cni-plugins
systemctl enable containerd

mv /tmp/10-containers.network /etc/systemd/network/
mkdir /etc/iproute2
mv /tmp/rt_tables /etc/iproute2/
chmod +x /tmp/hyphae-agent
mv /tmp/agent /usr/local/bin/hyphae-agent