targetScope = 'resourceGroup'

@description('Common project name.')
@minLength(2)
@maxLength(12)
param baseName string = 'qiankui'

@description('Azure region represented by this resource group.')
param location string = resourceGroup().location

@description('Stable lowercase region identifier used in resource names.')
param regionSlug string

@description('Address space reserved for this region. It must not overlap another Qiankui region.')
param networkAddressPrefix string

@description('Container Apps infrastructure subnet within networkAddressPrefix.')
param infrastructureSubnetAddressPrefix string

@description('Platform-managed resource group dedicated to this Container Apps environment.')
param infrastructureResourceGroupName string = '${baseName}-infra-${regionSlug}'

@description('Principal ID of the shared GitHub deployment identity.')
param githubPrincipalId string

var environmentName = '${baseName}-${regionSlug}'
var networkName = '${baseName}-vnet-${regionSlug}'
var subnetName = '${baseName}-snet-aca-${regionSlug}'
var regionalTags = {
  project: baseName
  managedBy: 'bicep'
  scope: 'regional'
  region: regionSlug
}
var containerAppsContributorRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '358470bc-b998-42bd-ab17-a7e34c199c0f')

resource network 'Microsoft.Network/virtualNetworks@2024-05-01' = {
  name: networkName
  location: location
  tags: regionalTags
  properties: {
    addressSpace: {
      addressPrefixes: [
        networkAddressPrefix
      ]
    }
    privateEndpointVNetPolicies: 'Disabled'
  }
}

resource infrastructureSubnet 'Microsoft.Network/virtualNetworks/subnets@2024-05-01' = {
  parent: network
  name: subnetName
  properties: {
    addressPrefix: infrastructureSubnetAddressPrefix
    delegations: [
      {
        name: 'container-apps'
        properties: {
          serviceName: 'Microsoft.App/environments'
        }
      }
    ]
    privateEndpointNetworkPolicies: 'Disabled'
    privateLinkServiceNetworkPolicies: 'Enabled'
  }
}

resource environment 'Microsoft.App/managedEnvironments@2025-01-01' = {
  name: environmentName
  location: location
  tags: regionalTags
  properties: {
    infrastructureResourceGroup: infrastructureResourceGroupName
    peerAuthentication: {
      mtls: {
        enabled: false
      }
    }
    peerTrafficConfiguration: {
      encryption: {
        enabled: false
      }
    }
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

resource githubContainerAppsContributor 'Microsoft.Authorization/roleAssignments@2022-04-01' = {
  name: guid(resourceGroup().id, githubPrincipalId, containerAppsContributorRole)
  properties: {
    roleDefinitionId: containerAppsContributorRole
    principalId: githubPrincipalId
    principalType: 'ServicePrincipal'
  }
}

output environmentName string = environment.name
output environmentDefaultDomain string = environment.properties.defaultDomain
output infrastructureResourceGroupName string = infrastructureResourceGroupName
output networkName string = network.name
output subnetName string = infrastructureSubnet.name
