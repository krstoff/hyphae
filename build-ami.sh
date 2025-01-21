#!/bin/bash
set -euo pipefail
pushd agent/
cargo build --release
popd
pushd packer/
packer build --var "commit-id=$(git rev-parse --short HEAD)" .
popd

