targetScope = 'resourceGroup'

@description('Common project name. Globally scoped services receive a deterministic suffix.')
@minLength(2)
@maxLength(12)
param baseName string = 'qiankui'

@description('Azure region for shared resources.')
param location string = resourceGroup().location

@description('GitHub repository in owner/name form.')
param githubRepository string

@description('GitHub environment allowed to request Azure OIDC tokens.')
param githubEnvironment string = 'production'

@description('Object ID allowed to bootstrap Key Vault secrets.')
param bootstrapPrincipalId string

var suffix = take(uniqueString(subscription().subscriptionId), 8)
var registryName = '${baseName}${suffix}'
var vaultName = '${baseName}-${suffix}'
var runtimeIdentityName = '${baseName}-runtime'
var githubIdentityName = '${baseName}-github'
var sharedTags = {
  project: baseName
  managedBy: 'bicep'
  scope: 'shared'
}
var acrPullRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '7f951dda-4ed3-4680-a7ca-43fe172d538d')
var acrPushRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '8311e382-0749-4cb8-b61a-304f252e45ec')
var keyVaultSecretsOfficerRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', 'b86a8fe4-44ce-4948-aee5-eccb2c155cd7')

resource registry 'Microsoft.ContainerRegistry/registries@2025-11-01' = {
  name: registryName
  location: location
  tags: sharedTags
  sku: {
    name: 'Basic'
  }
  properties: {
    adminUserEnabled: false
    dataEndpointEnabled: false
    encryption: {
      status: 'disabled'
    }
    networkRuleBypassAllowedForTasks: false
    publicNetworkAccess: 'Enabled'
    roleAssignmentMode: 'LegacyRegistryPermissions'
  }
}

resource vault 'Microsoft.KeyVault/vaults@2024-11-01' = {
  name: vaultName
  location: location
  tags: sharedTags
  properties: {
    tenantId: tenant().tenantId
    sku: {
      family: 'A'
      name: 'standard'
    }
    enableRbacAuthorization: true
    enablePurgeProtection: true
    softDeleteRetentionInDays: 7
    publicNetworkAccess: 'Enabled'
  }
}

resource runtimeIdentity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {
  name: runtimeIdentityName
  location: location
  tags: union(sharedTags, {
    purpose: 'runtime'
  })
}

resource githubIdentity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {
  name: githubIdentityName
  location: location
  tags: union(sharedTags, {
    purpose: 'deployment'
  })
}

resource githubFederation 'Microsoft.ManagedIdentity/userAssignedIdentities/federatedIdentityCredentials@2023-01-31' = {
  parent: githubIdentity
  name: 'github-${githubEnvironment}'
  properties: {
    issuer: 'https://token.actions.githubusercontent.com'
    subject: 'repo:${githubRepository}:environment:${githubEnvironment}'
    audiences: [
      'api://AzureADTokenExchange'
    ]
  }
}

resource runtimeAcrPull 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(registry.id, runtimeIdentity.id, acrPullRole)
  scope: registry
  properties: {
    roleDefinitionId: acrPullRole
    principalId: runtimeIdentity.properties.principalId
    principalType: 'ServicePrincipal'
  }
}

resource githubAcrPush 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(registry.id, githubIdentity.id, acrPushRole)
  scope: registry
  properties: {
    roleDefinitionId: acrPushRole
    principalId: githubIdentity.properties.principalId
    principalType: 'ServicePrincipal'
  }
}

resource bootstrapSecretsOfficer 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(vault.id, bootstrapPrincipalId, keyVaultSecretsOfficerRole)
  scope: vault
  properties: {
    roleDefinitionId: keyVaultSecretsOfficerRole
    principalId: bootstrapPrincipalId
    principalType: 'User'
  }
}

output registryName string = registry.name
output registryLoginServer string = registry.properties.loginServer
output keyVaultName string = vault.name
output keyVaultId string = vault.id
output runtimeIdentityName string = runtimeIdentity.name
output runtimeIdentityId string = runtimeIdentity.id
output runtimePrincipalId string = runtimeIdentity.properties.principalId
output githubClientId string = githubIdentity.properties.clientId
output githubPrincipalId string = githubIdentity.properties.principalId
