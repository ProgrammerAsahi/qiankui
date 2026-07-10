#!/usr/bin/env bash
set -euo pipefail
umask 077

BASE_NAME="qiankui"
SHARED_RESOURCE_GROUP=""
REGION="japaneast"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --shared-resource-group)
      SHARED_RESOURCE_GROUP="${2:-}"
      shift 2
      ;;
    --region)
      REGION="${2:-}"
      shift 2
      ;;
    --name)
      BASE_NAME="${2:-}"
      shift 2
      ;;
    -h|--help)
      printf 'Usage: deploy/azure/configure-client.sh [--region SLUG] [--shared-resource-group NAME] [--name NAME]\n'
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

if [[ -z "$SHARED_RESOURCE_GROUP" ]]; then
  SHARED_RESOURCE_GROUP="${BASE_NAME}-shared"
fi
if [[ ! "$REGION" =~ ^[a-z0-9][a-z0-9-]*$ ]]; then
  printf '%s\n' '--region must be a lowercase region slug' >&2
  exit 2
fi

REGIONAL_RESOURCE_GROUP="${BASE_NAME}-${REGION}"
APP_NAME="${BASE_NAME}-relay-${REGION}"

for command in az cargo git; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  }
done

ROOT="$(git rev-parse --show-toplevel)"
KEY_VAULT_NAME="$(az deployment group show \
  --resource-group "$SHARED_RESOURCE_GROUP" \
  --name "$BASE_NAME-shared" \
  --query properties.outputs.keyVaultName.value \
  --output tsv)"
RELAY_FQDN="$(az containerapp show \
  --resource-group "$REGIONAL_RESOURCE_GROUP" \
  --name "$APP_NAME" \
  --query properties.configuration.ingress.fqdn \
  --output tsv)"

printf '一、装近端命令\n'
cargo install \
  --path "$ROOT" \
  --bin qiankui \
  --locked \
  --force \
  --quiet
QIANKUI_BIN="$HOME/.cargo/bin/qiankui"

printf '二、取公证与符节于 Key Vault\n'
CONFIG_DIR="$HOME/.config/qiankui"
CA_PATH="$CONFIG_DIR/azure-ca.pem"
mkdir -p "$CONFIG_DIR"
chmod 700 "$CONFIG_DIR"
az keyvault secret show \
  --vault-name "$KEY_VAULT_NAME" \
  --name tls-ca-cert \
  --query value \
  --output tsv >"$CA_PATH"
chmod 600 "$CA_PATH"

TOKEN="$(az keyvault secret show \
  --vault-name "$KEY_VAULT_NAME" \
  --name relay-token \
  --query value \
  --output tsv)"
printf '%s' "$TOKEN" \
  | "$QIANKUI_BIN" config init \
      --relay "https://$RELAY_FQDN:8443" \
      --ca "$CA_PATH" \
      --token-stdin \
      --force
unset TOKEN

printf '\n近端已备，所择区域为 %s。运行 `qiankui`，本机应用取 SOCKS5 127.0.0.1:1080。\n' "$REGION"
