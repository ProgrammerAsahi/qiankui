targetScope = 'resourceGroup'

@minLength(2)
@maxLength(12)
param baseName string = 'qiankui'

@description('Stable lowercase region identifier used in regional secret names.')
param regionSlug string

param keyVaultName string

param runtimeIdentityName string = '${baseName}-runtime'

var tlsCertificateSecretName = 'tls-cert-${regionSlug}'
var tlsPrivateKeySecretName = 'tls-key-${regionSlug}'
var keyVaultSecretsUserRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '4633458b-17de-408a-b874-0445c86b69e6')

resource vault 'Microsoft.KeyVault/vaults@2024-11-01' existing = {
  name: keyVaultName
}

resource runtimeIdentity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' existing = {
  name: runtimeIdentityName
}

resource relayToken 'Microsoft.KeyVault/vaults/secrets@2024-11-01' existing = {
  parent: vault
  name: 'relay-token'
}

resource tlsCertificate 'Microsoft.KeyVault/vaults/secrets@2024-11-01' existing = {
  parent: vault
  name: tlsCertificateSecretName
}

resource tlsPrivateKey 'Microsoft.KeyVault/vaults/secrets@2024-11-01' existing = {
  parent: vault
  name: tlsPrivateKeySecretName
}

resource relayTokenReader 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(relayToken.id, runtimeIdentity.id, keyVaultSecretsUserRole)
  scope: relayToken
  properties: {
    roleDefinitionId: keyVaultSecretsUserRole
    principalId: runtimeIdentity.properties.principalId
    principalType: 'ServicePrincipal'
  }
}

resource tlsCertificateReader 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(tlsCertificate.id, runtimeIdentity.id, keyVaultSecretsUserRole)
  scope: tlsCertificate
  properties: {
    roleDefinitionId: keyVaultSecretsUserRole
    principalId: runtimeIdentity.properties.principalId
    principalType: 'ServicePrincipal'
  }
}

resource tlsPrivateKeyReader 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(tlsPrivateKey.id, runtimeIdentity.id, keyVaultSecretsUserRole)
  scope: tlsPrivateKey
  properties: {
    roleDefinitionId: keyVaultSecretsUserRole
    principalId: runtimeIdentity.properties.principalId
    principalType: 'ServicePrincipal'
  }
}
