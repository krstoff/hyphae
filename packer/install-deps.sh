#!/bin/sh
sudo dnf install containerd cni-plugins
sudo systemctl enable containerd