#!/usr/bin/env bash
set -euo pipefail
umask 077

BASE_NAME="qiankui"
SHARED_RESOURCE_GROUP=""
SELECTED_REGION=""

usage() {
  printf '%s\n' \
    "Usage: deploy/azure/rotate-token.sh [options]" \
    "" \
    "Options:" \
    "  --region SLUG           Restart only one configured region" \
    "  --shared-resource-group Shared resource group (default: <name>-shared)" \
    "  --name                  Common project name (default: qiankui)"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --region)
      SELECTED_REGION="${2:-}"
      shift 2
      ;;
    --shared-resource-group)
      SHARED_RESOURCE_GROUP="${2:-}"
      shift 2
      ;;
    --name)
      BASE_NAME="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$SHARED_RESOURCE_GROUP" ]]; then
  SHARED_RESOURCE_GROUP="${BASE_NAME}-shared"
fi
if [[ -n "$SELECTED_REGION" && ! "$SELECTED_REGION" =~ ^[a-z0-9][a-z0-9-]*$ ]]; then
  printf '%s\n' '--region must be a lowercase region slug' >&2
  exit 2
fi

for command in az git jq openssl; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  }
done

ROOT="$(git rev-parse --show-toplevel)"
REGIONS_FILE="$ROOT/deploy/azure/regions.json"
TEMPORARY="$(mktemp -d)"
trap 'rm -rf "$TEMPORARY"' EXIT

shared_output() {
  az deployment group show \
    --resource-group "$SHARED_RESOURCE_GROUP" \
    --name "$BASE_NAME-shared" \
    --query "properties.outputs.$1.value" \
    --output tsv
}

wait_for_revision() {
  local resource_group="$1"
  local app_name="$2"
  local revision="$3"
  local attempt health state
  for attempt in $(seq 1 30); do
    health="$(az containerapp revision show \
      --resource-group "$resource_group" \
      --name "$app_name" \
      --revision "$revision" \
      --query properties.healthState \
      --output tsv)"
    state="$(az containerapp revision show \
      --resource-group "$resource_group" \
      --name "$app_name" \
      --revision "$revision" \
      --query properties.runningState \
      --output tsv)"
    if [[ "$health" == "Healthy" \
      && ( "$state" == "Running" || "$state" == "RunningAtMaxScale" ) ]]; then
      return 0
    fi
    if [[ "$state" == "Failed" ]]; then
      break
    fi
    sleep 10
  done
  printf 'Revision %s did not become healthy after token rotation\n' "$revision" >&2
  return 1
}

KEY_VAULT_NAME="$(shared_output keyVaultName)"
RUNTIME_IDENTITY_ID="$(shared_output runtimeIdentityId)"

openssl rand -hex 32 | tr -d '\r\n' >"$TEMPORARY/relay-token"
TOKEN_SECRET_ID="$(az keyvault secret set \
  --vault-name "$KEY_VAULT_NAME" \
  --name relay-token \
  --file "$TEMPORARY/relay-token" \
  --content-type text/plain \
  --query id \
  --output tsv)"

matched=false
while IFS= read -r region_slug; do
  matched=true
  resource_group="${BASE_NAME}-${region_slug}"
  app_name="${BASE_NAME}-relay-${region_slug}"
  printf '易 %s 符节并重启置邮\n' "$region_slug"
  az containerapp secret set \
    --resource-group "$resource_group" \
    --name "$app_name" \
    --secrets "relay-token=keyvaultref:$TOKEN_SECRET_ID,identityref:$RUNTIME_IDENTITY_ID" \
    --output none
  revision="$(az containerapp show \
    --resource-group "$resource_group" \
    --name "$app_name" \
    --query properties.latestRevisionName \
    --output tsv)"
  az containerapp revision restart \
    --resource-group "$resource_group" \
    --name "$app_name" \
    --revision "$revision" \
    --output none
  wait_for_revision "$resource_group" "$app_name" "$revision"
done < <(jq -r --arg selected "$SELECTED_REGION" \
  '.regions[] | select($selected == "" or .slug == $selected) | .slug' \
  "$REGIONS_FILE")

if [[ "$matched" != true ]]; then
  printf 'Region not found in %s: %s\n' "$REGIONS_FILE" "$SELECTED_REGION" >&2
  exit 2
fi

printf '符节已易，所选 revision 皆已健康重启；请即运行 configure-client.sh 更新近端。\n'
