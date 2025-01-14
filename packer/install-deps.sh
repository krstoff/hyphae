#!/bin/sh
doas -u root su <<HERE
apk add containerd containerd-openrc
# debugging
apk add nano cri-tools dhclient
cat >/etc/modules-load.d/containerd.conf "overlay"
modprobe overlay
rc-update add containerd default
rc-service containerd start
HERE