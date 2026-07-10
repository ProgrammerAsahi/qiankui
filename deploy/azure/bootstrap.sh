#!/usr/bin/env bash
set -euo pipefail
umask 077

BASE_NAME="qiankui"
SHARED_RESOURCE_GROUP=""
GITHUB_ENVIRONMENT="production"
GITHUB_REPOSITORY=""
SELECTED_REGION=""

usage() {
  printf '%s\n' \
    "Usage: deploy/azure/bootstrap.sh --github-repository OWNER/REPOSITORY [options]" \
    "" \
    "Options:" \
    "  --github-repository     Repository allowed to deploy through OIDC" \
    "  --region SLUG           Deploy one entry from deploy/azure/regions.json" \
    "  --shared-resource-group Shared resource group (default: <name>-shared)" \
    "  --name                  Common project name (default: qiankui)"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --github-repository)
      GITHUB_REPOSITORY="${2:-}"
      shift 2
      ;;
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

if [[ ! "$GITHUB_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  printf '%s\n' '--github-repository must use OWNER/REPOSITORY form' >&2
  exit 2
fi
if [[ ! "$BASE_NAME" =~ ^[a-z][a-z0-9-]{1,11}$ ]]; then
  printf '%s\n' '--name must be 2-12 lowercase letters, digits, or hyphens' >&2
  exit 2
fi
if [[ -n "$SELECTED_REGION" && ! "$SELECTED_REGION" =~ ^[a-z0-9][a-z0-9-]*$ ]]; then
  printf '%s\n' '--region must be a lowercase region slug' >&2
  exit 2
fi
if [[ -z "$SHARED_RESOURCE_GROUP" ]]; then
  SHARED_RESOURCE_GROUP="${BASE_NAME}-shared"
fi

for command in az git jq openssl; do
  command -v "$command" >/dev/null || {
    printf 'Required command not found: %s\n' "$command" >&2
    exit 1
  }
done

ROOT="$(git rev-parse --show-toplevel)"
SHARED_TEMPLATE="$ROOT/deploy/azure/shared.bicep"
REGION_TEMPLATE="$ROOT/deploy/azure/region.bicep"
ACCESS_TEMPLATE="$ROOT/deploy/azure/secret-access.bicep"
APP_TEMPLATE="$ROOT/deploy/azure/app.bicep"
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

region_output() {
  az deployment group show \
    --resource-group "$1" \
    --name "$BASE_NAME-region-$2" \
    --query "properties.outputs.$3.value" \
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
  printf 'Container App revision %s did not become healthy\n' "$revision" >&2
  return 1
}

issue_region_certificate() {
  local region_slug="$1"
  local relay_fqdn="$2"
  local certificate_dir="$TEMPORARY/$region_slug"
  mkdir -p "$certificate_dir"

  openssl genpkey \
    -algorithm RSA \
    -pkeyopt rsa_keygen_bits:2048 \
    -out "$certificate_dir/server-key.pem"
  openssl req \
    -new \
    -key "$certificate_dir/server-key.pem" \
    -subj "/CN=$BASE_NAME-$region_slug" \
    -out "$certificate_dir/server.csr"
  printf '%s\n' \
    "subjectAltName=DNS:$relay_fqdn" \
    'basicConstraints=critical,CA:FALSE' \
    'keyUsage=critical,digitalSignature,keyEncipherment' \
    'extendedKeyUsage=serverAuth' >"$certificate_dir/server.ext"
  openssl x509 \
    -req \
    -in "$certificate_dir/server.csr" \
    -CA "$TEMPORARY/ca-cert.pem" \
    -CAkey "$TEMPORARY/ca-key.pem" \
    -CAcreateserial \
    -sha256 \
    -days 365 \
    -extfile "$certificate_dir/server.ext" \
    -out "$certificate_dir/server-cert.pem"
  cp "$certificate_dir/server-cert.pem" "$certificate_dir/server-fullchain.pem"
  printf '\n' >>"$certificate_dir/server-fullchain.pem"
  sed -n '1,$p' "$TEMPORARY/ca-cert.pem" >>"$certificate_dir/server-fullchain.pem"

  put_secret_file \
    "$KEY_VAULT_NAME" \
    "tls-cert-$region_slug" \
    "$certificate_dir/server-fullchain.pem" \
    'application/x-pem-file'
  put_secret_file \
    "$KEY_VAULT_NAME" \
    "tls-key-$region_slug" \
    "$certificate_dir/server-key.pem" \
    'application/x-pem-file'
}

deploy_region() {
  local region_json="$1"
  local region_slug location network_prefix subnet_prefix
  local regional_resource_group infrastructure_resource_group
  local environment_domain relay_fqdn app_name actual_fqdn revision
  local deployed attempt

  region_slug="$(jq -r '.slug' <<<"$region_json")"
  location="$(jq -r '.location' <<<"$region_json")"
  network_prefix="$(jq -r '.networkAddressPrefix' <<<"$region_json")"
  subnet_prefix="$(jq -r '.infrastructureSubnetAddressPrefix' <<<"$region_json")"
  regional_resource_group="${BASE_NAME}-${region_slug}"
  infrastructure_resource_group="${BASE_NAME}-infra-${region_slug}"
  app_name="${BASE_NAME}-relay-${region_slug}"

  printf '\n三、立区域资源（%s）\n' "$region_slug"
  az group create \
    --name "$regional_resource_group" \
    --location "$location" \
    --tags project="$BASE_NAME" managedBy=bicep scope=regional region="$region_slug" \
    --output none
  az deployment group create \
    --resource-group "$regional_resource_group" \
    --name "$BASE_NAME-region-$region_slug" \
    --template-file "$REGION_TEMPLATE" \
    --parameters \
      baseName="$BASE_NAME" \
      location="$location" \
      regionSlug="$region_slug" \
      networkAddressPrefix="$network_prefix" \
      infrastructureSubnetAddressPrefix="$subnet_prefix" \
      infrastructureResourceGroupName="$infrastructure_resource_group" \
      githubPrincipalId="$GITHUB_PRINCIPAL_ID" \
    --output none

  environment_domain="$(region_output "$regional_resource_group" "$region_slug" environmentDefaultDomain)"
  relay_fqdn="$app_name.$environment_domain"

  printf '四、为 %s 署 TLS 信证\n' "$region_slug"
  issue_region_certificate "$region_slug" "$relay_fqdn"
  az deployment group create \
    --resource-group "$SHARED_RESOURCE_GROUP" \
    --name "$BASE_NAME-secret-access-$region_slug" \
    --template-file "$ACCESS_TEMPLATE" \
    --parameters \
      baseName="$BASE_NAME" \
      regionSlug="$region_slug" \
      keyVaultName="$KEY_VAULT_NAME" \
      runtimeIdentityName="$RUNTIME_IDENTITY_NAME" \
    --output none

  printf '五、起 %s 置邮\n' "$region_slug"
  deployed=false
  for attempt in $(seq 1 12); do
    if az deployment group create \
      --resource-group "$regional_resource_group" \
      --name "$BASE_NAME-app-$region_slug" \
      --template-file "$APP_TEMPLATE" \
      --parameters \
        baseName="$BASE_NAME" \
        location="$location" \
        regionSlug="$region_slug" \
        sharedResourceGroupName="$SHARED_RESOURCE_GROUP" \
        registryName="$REGISTRY_NAME" \
        keyVaultName="$KEY_VAULT_NAME" \
        runtimeIdentityName="$RUNTIME_IDENTITY_NAME" \
        image="$IMAGE" \
      --output none; then
      deployed=true
      break
    fi
    printf '待 RBAC 生效，稍后重试（%s/12）\n' "$attempt" >&2
    sleep 15
  done
  if [[ "$deployed" != true ]]; then
    printf 'Container App deployment did not become ready in %s\n' "$region_slug" >&2
    return 1
  fi

  actual_fqdn="$(az containerapp show \
    --resource-group "$regional_resource_group" \
    --name "$app_name" \
    --query properties.configuration.ingress.fqdn \
    --output tsv)"
  if [[ "$actual_fqdn" != "$relay_fqdn" ]]; then
    printf 'Unexpected relay FQDN: expected %s, received %s\n' "$relay_fqdn" "$actual_fqdn" >&2
    return 1
  fi
  revision="$(az containerapp show \
    --resource-group "$regional_resource_group" \
    --name "$app_name" \
    --query properties.latestRevisionName \
    --output tsv)"
  wait_for_revision "$regional_resource_group" "$app_name" "$revision"
  printf '潜逵置邮已立（%s）：https://%s:8443\n' "$region_slug" "$actual_fqdn"
}

jq -e '
  .schemaVersion == 1
  and (.sharedLocation | type == "string" and length > 0)
  and (.regions | type == "array" and length > 0)
  and ([.regions[].slug] | length == (unique | length))
  and all(.regions[];
    (.slug | type == "string" and test("^[a-z0-9][a-z0-9-]*$"))
    and (.location | type == "string" and length > 0)
    and (.networkAddressPrefix | type == "string" and length > 0)
    and (.infrastructureSubnetAddressPrefix | type == "string" and length > 0)
  )
' "$REGIONS_FILE" >/dev/null

if [[ -n "$SELECTED_REGION" ]]; then
  REGION_COUNT="$(jq --arg slug "$SELECTED_REGION" '[.regions[] | select(.slug == $slug)] | length' "$REGIONS_FILE")"
  if [[ "$REGION_COUNT" -ne 1 ]]; then
    printf 'Region not found in %s: %s\n' "$REGIONS_FILE" "$SELECTED_REGION" >&2
    exit 2
  fi
fi
SHARED_LOCATION="$(jq -r '.sharedLocation' "$REGIONS_FILE")"

az account show --output none
BOOTSTRAP_PRINCIPAL_ID="$(az ad signed-in-user show --query id --output tsv)"

printf '一、立共享资源组与共用资源（%s）\n' "$SHARED_RESOURCE_GROUP"
az group create \
  --name "$SHARED_RESOURCE_GROUP" \
  --location "$SHARED_LOCATION" \
  --tags project="$BASE_NAME" managedBy=bicep scope=shared \
  --output none
az deployment group create \
  --resource-group "$SHARED_RESOURCE_GROUP" \
  --name "$BASE_NAME-shared" \
  --template-file "$SHARED_TEMPLATE" \
  --parameters \
    baseName="$BASE_NAME" \
    location="$SHARED_LOCATION" \
    githubRepository="$GITHUB_REPOSITORY" \
    githubEnvironment="$GITHUB_ENVIRONMENT" \
    bootstrapPrincipalId="$BOOTSTRAP_PRINCIPAL_ID" \
  --output none

REGISTRY_NAME="$(shared_output registryName)"
REGISTRY_LOGIN_SERVER="$(shared_output registryLoginServer)"
KEY_VAULT_NAME="$(shared_output keyVaultName)"
RUNTIME_IDENTITY_NAME="$(shared_output runtimeIdentityName)"
GITHUB_PRINCIPAL_ID="$(shared_output githubPrincipalId)"

printf '二、备共享符节、公证并构建 relay 镜像\n'
wait_for_vault_access "$KEY_VAULT_NAME"
if ! secret_exists "$KEY_VAULT_NAME" relay-token; then
  openssl rand -hex 32 | tr -d '\r\n' >"$TEMPORARY/relay-token"
  put_secret_file "$KEY_VAULT_NAME" relay-token "$TEMPORARY/relay-token" 'text/plain'
fi

CA_CERT_EXISTS=false
CA_KEY_EXISTS=false
secret_exists "$KEY_VAULT_NAME" tls-ca-cert && CA_CERT_EXISTS=true
secret_exists "$KEY_VAULT_NAME" tls-ca-key && CA_KEY_EXISTS=true
if [[ "$CA_CERT_EXISTS" != "$CA_KEY_EXISTS" ]]; then
  printf 'Key Vault contains only one half of the TLS CA; refusing to replace it automatically\n' >&2
  exit 1
fi
if [[ "$CA_CERT_EXISTS" == false ]]; then
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
  put_secret_file "$KEY_VAULT_NAME" tls-ca-cert "$TEMPORARY/ca-cert.pem" 'application/x-pem-file'
  put_secret_file "$KEY_VAULT_NAME" tls-ca-key "$TEMPORARY/ca-key.pem" 'application/x-pem-file'
else
  az keyvault secret show \
    --vault-name "$KEY_VAULT_NAME" \
    --name tls-ca-cert \
    --query value \
    --output tsv >"$TEMPORARY/ca-cert.pem"
  az keyvault secret show \
    --vault-name "$KEY_VAULT_NAME" \
    --name tls-ca-key \
    --query value \
    --output tsv >"$TEMPORARY/ca-key.pem"
fi

IMAGE_TAG="bootstrap-$(date -u +%Y%m%d%H%M%S)"
az acr build \
  --registry "$REGISTRY_NAME" \
  --image "$BASE_NAME-relay:$IMAGE_TAG" \
  --file "$ROOT/Dockerfile" \
  "$ROOT" \
  --output none
IMAGE="$REGISTRY_LOGIN_SERVER/$BASE_NAME-relay:$IMAGE_TAG"

while IFS= read -r region_json; do
  deploy_region "$region_json"
done < <(jq -c --arg selected "$SELECTED_REGION" \
  '.regions[] | select($selected == "" or .slug == $selected)' \
  "$REGIONS_FILE")

printf '\n继而运行：deploy/azure/configure-github.sh --github-repository %s\n' "$GITHUB_REPOSITORY"
if [[ -n "$SELECTED_REGION" ]]; then
  printf '再运行：deploy/azure/configure-client.sh --region %s\n' "$SELECTED_REGION"
else
  printf '再运行：deploy/azure/configure-client.sh --region <slug>\n'
fi
