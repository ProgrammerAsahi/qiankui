#!/usr/bin/env bash
set -euo pipefail

BASE_NAME="qiankui"
RESOURCE_GROUP="qiankui"
GITHUB_ENVIRONMENT="production"
GITHUB_REPOSITORY=""

usage() {
  printf '%s\n' \
    "Usage: deploy/azure/configure-github.sh --github-repository OWNER/REPOSITORY" \
    "" \
    "The script writes only Azure identifiers to GitHub environment secrets." \
    "No client secret is created; deployments authenticate through OIDC."
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --github-repository)
      GITHUB_REPOSITORY="${2:-}"
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
      exit 2
      ;;
  esac
done

if [[ ! "$GITHUB_REPOSITORY" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]]; then
  printf '%s\n' '--github-repository must use OWNER/REPOSITORY form' >&2
  exit 2
fi

command -v az >/dev/null || { printf 'Required command not found: az\n' >&2; exit 1; }
command -v gh >/dev/null || { printf 'Required command not found: gh\n' >&2; exit 1; }
gh auth status --hostname github.com >/dev/null

GITHUB_CLIENT_ID="$(az deployment group show \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME-base" \
  --query properties.outputs.githubClientId.value \
  --output tsv)"
AZURE_ACR_NAME="$(az deployment group show \
  --resource-group "$RESOURCE_GROUP" \
  --name "$BASE_NAME-base" \
  --query properties.outputs.registryName.value \
  --output tsv)"
AZURE_TENANT_ID="$(az account show --query tenantId --output tsv)"
AZURE_SUBSCRIPTION_ID="$(az account show --query id --output tsv)"

gh api \
  --method PUT \
  "repos/$GITHUB_REPOSITORY/environments/$GITHUB_ENVIRONMENT" \
  -F 'deployment_branch_policy[protected_branches]=false' \
  -F 'deployment_branch_policy[custom_branch_policies]=true' \
  --silent
MAIN_POLICY_ID="$(gh api \
  "repos/$GITHUB_REPOSITORY/environments/$GITHUB_ENVIRONMENT/deployment-branch-policies" \
  --jq '.branch_policies[] | select(.name == "main" and .type == "branch") | .id')"
if [[ -z "$MAIN_POLICY_ID" ]]; then
  gh api \
    --method POST \
    "repos/$GITHUB_REPOSITORY/environments/$GITHUB_ENVIRONMENT/deployment-branch-policies" \
    -f name=main \
    -f type=branch \
    --silent
fi
printf '%s' "$GITHUB_CLIENT_ID" \
  | gh secret set AZURE_CLIENT_ID --repo "$GITHUB_REPOSITORY" --env "$GITHUB_ENVIRONMENT"
printf '%s' "$AZURE_TENANT_ID" \
  | gh secret set AZURE_TENANT_ID --repo "$GITHUB_REPOSITORY" --env "$GITHUB_ENVIRONMENT"
printf '%s' "$AZURE_SUBSCRIPTION_ID" \
  | gh secret set AZURE_SUBSCRIPTION_ID --repo "$GITHUB_REPOSITORY" --env "$GITHUB_ENVIRONMENT"
gh variable set AZURE_ACR_NAME \
  --repo "$GITHUB_REPOSITORY" \
  --env "$GITHUB_ENVIRONMENT" \
  --body "$AZURE_ACR_NAME"

printf 'GitHub %s 环境已接 Azure OIDC，且惟 main 可用；未造客户端密码。\n' "$GITHUB_ENVIRONMENT"
gh secret list --repo "$GITHUB_REPOSITORY" --env "$GITHUB_ENVIRONMENT"
gh variable list --repo "$GITHUB_REPOSITORY" --env "$GITHUB_ENVIRONMENT"
