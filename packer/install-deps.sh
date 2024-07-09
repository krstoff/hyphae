#!/bin/sh
sudo su root
sudo apt install --assume-yes containerd
sudo systemctl enable containerd

