pushd terraform/
address=$(terraform output -json | jq -r '.instances.value | to_entries | map(.value) [0]')
ssh -i ~/downloads/skeleton-key.pem ec2-user@"${address}"
popd
