#!/bin/bash
pushd packer/
packer build --var "commit-id=$(git rev-parse --short HEAD)" .
popd

