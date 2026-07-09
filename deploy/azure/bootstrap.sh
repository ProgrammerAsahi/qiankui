#!/usr/bin/env bash
set -euo pipefail
umask 077

BASE_NAME="qiankui"
LOCATION="japaneast"
RESOURCE_GROUP="qiankui"
GITHUB_ENVIRONMENT="production"
GITHUB_REPOSITORY=""

usage() {
  printf '%s\n' \
    "Usage: deploy/azure/bootstrap.sh --github-repository OWNER/REPOSITORY" \
    "" \
    "Options:" \
    "  --github-repository  Repository allowed to deploy through OIDC" \
    "  --location           Azure region (default: japaneast)" \
    "  --resource-group     Resource group (default: qiankui)" \
    "  --name               Common resource name (default: qiankui)"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --github-repository)
      GITHUB_REPOSITORY="${2:-}"
      shift 2
      ;;
    --location)
      LOCATION="${2:-}"
      shift 2
      ;;
    --resource-group)
      RESOURCE_GROUP="${2:-}"
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

if [[ ! "$GITHUB_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  printf '%s\n' '--github-repository must use OWNER/REPOSITORY form' >&2
  exit 2
fi

for command in az git openssl; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  }
done

ROOT="$(git rev-parse --show-toplevel)"
BASE_TEMPLATE="$ROOT/deploy/azure/base.bicep"
ACCESS_TEMPLATE="$ROOT/deploy/azure/secret-access.bicep"
APP_TEMPLATE="$ROOT/deploy/azure/app.bicep"
TEMPORARY="$(mktemp -d)"
trap 'rm -rf "$TEMPORARY"' EXIT

deployment_output() {
  az deployment group show \
    --resource-group "$RESOURCE_GROUP" \
    --name "$BASE_NAME-base" \
    --query "properties.outputs.$1.value" \
    --output tsv
}

wait_for_vault_access() {
  local attempt
  for attempt in $(seq 1 30); do
    if az keyvault secret list --vault-name "$1" --maxresults 1 --output none 2>/dev/null; then
      return 0
    fi
    sleep 10
  done
  printf 'Timed out waiting for Key Vault data-plane access\n' >&2
  return 1
}

secret_exists() {
  az keyvault secret show \
    --vault-name "$1" \
    --name "$2" \
    --query id \
    --output tsv >/dev/null 2>&1
}

put_secret_file() {
  az keyvault secret set \
    --vault-name "$1" \
    --name "$2" \
    --file "$3" \
    --content-type "$4" \
    --output none
}

az account show --output none
BOOTSTRAP_PRINCIPAL_ID="$(az ad signed-in-user show --query id --output tsv)"

printf '一、立资源组与基础设施（%s）\n' "$LOCATION"
az group create \
  --name "$RESOURCE_GROUP" \
  --location "$LOCATION" \
  --tags project="$BASE_NAME" managedBy=bicep \
  --output none

az deployment group create \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME-base" \
  --template-file "$BASE_TEMPLATE" \
  --parameters \
    baseName="$BASE_NAME" \
    location="$LOCATION" \
    githubRepository="$GITHUB_REPOSITORY" \
    githubEnvironment="$GITHUB_ENVIRONMENT" \
    bootstrapPrincipalId="$BOOTSTRAP_PRINCIPAL_ID" \
  --output none

REGISTRY_NAME="$(deployment_output registryName)"
REGISTRY_LOGIN_SERVER="$(deployment_output registryLoginServer)"
KEY_VAULT_NAME="$(deployment_output keyVaultName)"
ENVIRONMENT_DOMAIN="$(deployment_output environmentDefaultDomain)"
RELAY_FQDN="$BASE_NAME.$ENVIRONMENT_DOMAIN"

printf '二、云端构建 relay 镜像\n'
IMAGE_TAG="bootstrap-$(date -u +%Y%m%d%H%M%S)"
az acr build \
  --registry "$REGISTRY_NAME" \
  --image "$BASE_NAME-relay:$IMAGE_TAG" \
  --file "$ROOT/Dockerfile" \
  "$ROOT" \
  --output none
IMAGE="$REGISTRY_LOGIN_SERVER/$BASE_NAME-relay:$IMAGE_TAG"

printf '三、备符节与 TLS 信证于 Key Vault\n'
wait_for_vault_access "$KEY_VAULT_NAME"

if ! secret_exists "$KEY_VAULT_NAME" relay-token; then
  openssl rand -hex 32 | tr -d '\r\n' >"$TEMPORARY/relay-token"
  put_secret_file "$KEY_VAULT_NAME" relay-token "$TEMPORARY/relay-token" 'text/plain'
fi

if ! secret_exists "$KEY_VAULT_NAME" tls-ca-cert \
  || ! secret_exists "$KEY_VAULT_NAME" tls-ca-key \
  || ! secret_exists "$KEY_VAULT_NAME" tls-cert \
  || ! secret_exists "$KEY_VAULT_NAME" tls-key; then
  openssl genpkey \
    -algorithm RSA \
    -pkeyopt rsa_keygen_bits:3072 \
    -out "$TEMPORARY/ca-key.pem"
  openssl req \
    -x509 \
    -new \
    -key "$TEMPORARY/ca-key.pem" \
    -sha256 \
    -days 3650 \
    -subj '/CN=Qiankui Root CA' \
    -addext 'basicConstraints=critical,CA:TRUE,pathlen:0' \
    -addext 'keyUsage=critical,keyCertSign,cRLSign' \
    -out "$TEMPORARY/ca-cert.pem"
  openssl genpkey \
    -algorithm RSA \
    -pkeyopt rsa_keygen_bits:2048 \
    -out "$TEMPORARY/server-key.pem"
  openssl req \
    -new \
    -key "$TEMPORARY/server-key.pem" \
    -subj "/CN=$RELAY_FQDN" \
    -out "$TEMPORARY/server.csr"
  printf '%s\n' \
    "subjectAltName=DNS:$RELAY_FQDN" \
    'basicConstraints=critical,CA:FALSE' \
    'keyUsage=critical,digitalSignature,keyEncipherment' \
    'extendedKeyUsage=serverAuth' >"$TEMPORARY/server.ext"
  openssl x509 \
    -req \
    -in "$TEMPORARY/server.csr" \
    -CA "$TEMPORARY/ca-cert.pem" \
    -CAkey "$TEMPORARY/ca-key.pem" \
    -CAcreateserial \
    -sha256 \
    -days 365 \
    -extfile "$TEMPORARY/server.ext" \
    -out "$TEMPORARY/server-cert.pem"
  cp "$TEMPORARY/server-cert.pem" "$TEMPORARY/server-fullchain.pem"
  printf '\n' >>"$TEMPORARY/server-fullchain.pem"
  sed -n '1,$p' "$TEMPORARY/ca-cert.pem" >>"$TEMPORARY/server-fullchain.pem"

  put_secret_file "$KEY_VAULT_NAME" tls-ca-cert "$TEMPORARY/ca-cert.pem" 'application/x-pem-file'
  put_secret_file "$KEY_VAULT_NAME" tls-ca-key "$TEMPORARY/ca-key.pem" 'application/x-pem-file'
  put_secret_file "$KEY_VAULT_NAME" tls-cert "$TEMPORARY/server-fullchain.pem" 'application/x-pem-file'
  put_secret_file "$KEY_VAULT_NAME" tls-key "$TEMPORARY/server-key.pem" 'application/x-pem-file'
fi

az deployment group create \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME-secret-access" \
  --template-file "$ACCESS_TEMPLATE" \
  --parameters baseName="$BASE_NAME" \
  --output none

printf '四、起 Container App\n'
deployed=false
for attempt in $(seq 1 12); do
  if az deployment group create \
    --resource-group "$RESOURCE_GROUP" \
    --name "$BASE_NAME-app" \
    --template-file "$APP_TEMPLATE" \
    --parameters baseName="$BASE_NAME" location="$LOCATION" image="$IMAGE" \
    --output none; then
    deployed=true
    break
  fi
  printf '待 RBAC 生效，稍后重试（%s/12）\n' "$attempt" >&2
  sleep 15
done

if [[ "$deployed" != true ]]; then
  printf 'Container App deployment did not become ready\n' >&2
  exit 1
fi

ACTUAL_FQDN="$(az containerapp show \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME" \
  --query properties.configuration.ingress.fqdn \
  --output tsv)"
if [[ "$ACTUAL_FQDN" != "$RELAY_FQDN" ]]; then
  printf 'Unexpected relay FQDN: expected %s, received %s\n' "$RELAY_FQDN" "$ACTUAL_FQDN" >&2
  exit 1
fi

REVISION="$(az containerapp show \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME" \
  --query properties.latestRevisionName \
  --output tsv)"
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
  printf 'Container App revision %s did not become healthy\n' "$REVISION" >&2
  exit 1
fi

printf '\n潜逵置邮已立：https://%s:8443\n' "$ACTUAL_FQDN"
printf '继而运行：deploy/azure/configure-github.sh --github-repository %s\n' "$GITHUB_REPOSITORY"
printf '再运行：deploy/azure/configure-client.sh\n'
