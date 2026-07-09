targetScope = 'resourceGroup'

@minLength(2)
@maxLength(12)
param baseName string = 'qiankui'

param location string = resourceGroup().location

@description('Immutable ACR image reference, preferably tagged with a Git commit SHA.')
param image string

var suffix = take(uniqueString(subscription().subscriptionId), 8)
var registryName = '${baseName}${suffix}'
var vaultName = '${baseName}-${suffix}'

resource environment 'Microsoft.App/managedEnvironments@2025-01-01' existing = {
  name: baseName
}

resource registry 'Microsoft.ContainerRegistry/registries@2025-11-01' existing = {
  name: registryName
}

resource vault 'Microsoft.KeyVault/vaults@2024-11-01' existing = {
  name: vaultName
}

resource runtimeIdentity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' existing = {
  name: baseName
}

resource app 'Microsoft.App/containerApps@2025-01-01' = {
  name: baseName
  location: location
  tags: {
    project: baseName
    managedBy: 'bicep-and-github-actions'
  }
  identity: {
    type: 'UserAssigned'
    userAssignedIdentities: {
      '${runtimeIdentity.id}': {}
    }
  }
  properties: {
    environmentId: environment.id
    workloadProfileName: 'Consumption'
    configuration: {
      activeRevisionsMode: 'Single'
      maxInactiveRevisions: 2
      registries: [
        {
          server: registry.properties.loginServer
          identity: runtimeIdentity.id
        }
      ]
      secrets: [
        {
          name: 'relay-token'
          keyVaultUrl: '${vault.properties.vaultUri}secrets/relay-token'
          identity: runtimeIdentity.id
        }
        {
          name: 'tls-cert'
          keyVaultUrl: '${vault.properties.vaultUri}secrets/tls-cert'
          identity: runtimeIdentity.id
        }
        {
          name: 'tls-key'
          keyVaultUrl: '${vault.properties.vaultUri}secrets/tls-key'
          identity: runtimeIdentity.id
        }
      ]
      ingress: {
        external: true
        transport: 'tcp'
        targetPort: 8443
        exposedPort: 8443
        allowInsecure: false
        traffic: [
          {
            latestRevision: true
            weight: 100
          }
        ]
      }
    }
    template: {
      containers: [
        {
          name: baseName
          image: image
          args: [
            '--listen'
            '0.0.0.0:8443'
            '--cert'
            '/mnt/secrets/tls-cert.pem'
            '--key'
            '/mnt/secrets/tls-key.pem'
            '--ports'
            '80,443'
          ]
          env: [
            {
              name: 'QIANKUI_TOKEN'
              secretRef: 'relay-token'
            }
          ]
          resources: {
            cpu: json('0.25')
            memory: '0.5Gi'
          }
          volumeMounts: [
            {
              volumeName: 'secrets'
              mountPath: '/mnt/secrets'
            }
          ]
          probes: [
            {
              type: 'Liveness'
              tcpSocket: {
                port: 8443
              }
              initialDelaySeconds: 5
              periodSeconds: 30
              timeoutSeconds: 3
              failureThreshold: 3
            }
            {
              type: 'Readiness'
              tcpSocket: {
                port: 8443
              }
              initialDelaySeconds: 2
              periodSeconds: 10
              timeoutSeconds: 3
              failureThreshold: 3
              successThreshold: 1
            }
          ]
        }
      ]
      scale: {
        minReplicas: 1
        maxReplicas: 1
      }
      volumes: [
        {
          name: 'secrets'
          storageType: 'Secret'
          secrets: [
            {
              secretRef: 'tls-cert'
              path: 'tls-cert.pem'
            }
            {
              secretRef: 'tls-key'
              path: 'tls-key.pem'
            }
          ]
        }
      ]
    }
  }
}

output fqdn string = app.properties.configuration.ingress.fqdn
output endpoint string = 'https://${app.properties.configuration.ingress.fqdn}:8443'
