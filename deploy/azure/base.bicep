targetScope = 'resourceGroup'

@description('Common resource name. Globally scoped services receive a deterministic suffix.')
@minLength(2)
@maxLength(12)
param baseName string = 'qiankui'

@description('Azure region for all regional resources.')
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
var acrPullRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '7f951dda-4ed3-4680-a7ca-43fe172d538d')
var acrPushRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '8311e382-0749-4cb8-b61a-304f252e45ec')
var containerAppsContributorRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '358470bc-b998-42bd-ab17-a7e34c199c0f')
var keyVaultSecretsOfficerRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', 'b86a8fe4-44ce-4948-aee5-eccb2c155cd7')

resource network 'Microsoft.Network/virtualNetworks@2024-05-01' = {
  name: baseName
  location: location
  tags: {
    project: baseName
    managedBy: 'bicep'
  }
  properties: {
    addressSpace: {
      addressPrefixes: [
        '10.42.0.0/24'
      ]
    }
  }
}

resource infrastructureSubnet 'Microsoft.Network/virtualNetworks/subnets@2024-05-01' = {
  parent: network
  name: baseName
  properties: {
    addressPrefix: '10.42.0.0/27'
    delegations: [
      {
        name: 'container-apps'
        properties: {
          serviceName: 'Microsoft.App/environments'
        }
      }
    ]
  }
}

resource environment 'Microsoft.App/managedEnvironments@2025-01-01' = {
  name: baseName
  location: location
  tags: {
    project: baseName
    managedBy: 'bicep'
  }
  properties: {
    vnetConfiguration: {
      infrastructureSubnetId: infrastructureSubnet.id
      internal: false
    }
    workloadProfiles: [
      {
        name: 'Consumption'
        workloadProfileType: 'Consumption'
      }
    ]
    zoneRedundant: false
  }
}

resource registry 'Microsoft.ContainerRegistry/registries@2025-11-01' = {
  name: registryName
  location: location
  tags: {
    project: baseName
    managedBy: 'bicep'
  }
  sku: {
    name: 'Basic'
  }
  properties: {
    adminUserEnabled: false
    publicNetworkAccess: 'Enabled'
    roleAssignmentMode: 'LegacyRegistryPermissions'
  }
}

resource vault 'Microsoft.KeyVault/vaults@2024-11-01' = {
  name: vaultName
  location: location
  tags: {
    project: baseName
    managedBy: 'bicep'
  }
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
  name: baseName
  location: location
  tags: {
    project: baseName
    purpose: 'runtime'
  }
}

resource githubIdentity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {
  name: '${baseName}-github'
  location: location
  tags: {
    project: baseName
    purpose: 'deployment'
  }
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

resource githubContainerAppsContributor 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(resourceGroup().id, githubIdentity.id, containerAppsContributorRole)
  properties: {
    roleDefinitionId: containerAppsContributorRole
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
output environmentDefaultDomain string = environment.properties.defaultDomain
output runtimeIdentityId string = runtimeIdentity.id
output runtimePrincipalId string = runtimeIdentity.properties.principalId
output githubClientId string = githubIdentity.properties.clientId
