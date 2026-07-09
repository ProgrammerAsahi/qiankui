#!/usr/bin/env bash
set -euo pipefail
umask 077

BASE_NAME="qiankui"
RESOURCE_GROUP="qiankui"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --resource-group)
      RESOURCE_GROUP="${2:-}"
      shift 2
      ;;
    --name)
      BASE_NAME="${2:-}"
      shift 2
      ;;
    -h|--help)
      printf 'Usage: deploy/azure/rotate-token.sh [--resource-group NAME] [--name NAME]\n'
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

for command in az openssl tr; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  }
done

TEMPORARY="$(mktemp -d)"
trap 'rm -rf "$TEMPORARY"' EXIT
KEY_VAULT_NAME="$(az deployment group show \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME-base" \
  --query properties.outputs.keyVaultName.value \
  --output tsv)"

openssl rand -hex 32 | tr -d '\r\n' >"$TEMPORARY/relay-token"
TOKEN_SECRET_ID="$(az keyvault secret set \
  --vault-name "$KEY_VAULT_NAME" \
  --name relay-token \
  --file "$TEMPORARY/relay-token" \
  --content-type text/plain \
  --query id \
  --output tsv)"
RUNTIME_IDENTITY_ID="$(az deployment group show \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME-base" \
  --query properties.outputs.runtimeIdentityId.value \
  --output tsv)"
az containerapp secret set \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME" \
  --secrets "relay-token=keyvaultref:$TOKEN_SECRET_ID,identityref:$RUNTIME_IDENTITY_ID" \
  --output none

REVISION="$(az containerapp show \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME" \
  --query properties.latestRevisionName \
  --output tsv)"
az containerapp revision restart \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME" \
  --revision "$REVISION" \
  --output none

healthy=false
for attempt in $(seq 1 30); do
  HEALTH="$(az containerapp revision show \
    --resource-group "$RESOURCE_GROUP" \
    --name "$BASE_NAME" \
    --revision "$REVISION" \
    --query properties.healthState \
    --output tsv)"
  STATE="$(az containerapp revision show \
    --resource-group "$RESOURCE_GROUP" \
    --name "$BASE_NAME" \
    --revision "$REVISION" \
    --query properties.runningState \
    --output tsv)"
  if [[ "$HEALTH" == "Healthy" \
    && ( "$STATE" == "Running" || "$STATE" == "RunningAtMaxScale" ) ]]; then
    healthy=true
    break
  fi
  if [[ "$STATE" == "Failed" ]]; then
    break
  fi
  sleep 10
done
if [[ "$healthy" != true ]]; then
  printf 'Revision %s did not become healthy after token rotation\n' "$REVISION" >&2
  exit 1
fi

printf '符节已易，revision 已健康重启；请即运行 deploy/azure/configure-client.sh 更新近端。\n'
