# Azure 置邮之法

此章以 Azure Container Apps 布潜逵置邮。其制分三层：共用之物归 `shared`，随地而异者归区域资源组，平台自理之负载均衡器与公网地址归该区域独有的 `infra` 资源组。镜像只构建一次，而后发往诸区域。

运行面为 Container Apps Consumption，公网入口为原始 TCP `8443`。镜像藏于 ACR，符节与 TLS 信证藏于 Key Vault；GitHub Actions 借 OIDC 短期登录，不设客户端密码。

## 名制

以日本东部 `japaneast` 为例：

| 层次 | 资源 | 名称 |
|---|---|---|
| 共用 | Resource Group | `qiankui-shared` |
| 共用 | ACR | `qiankui<订阅散列>` |
| 共用 | Key Vault | `qiankui-<订阅散列>` |
| 共用 | 运行身份 | `qiankui-runtime` |
| 共用 | GitHub OIDC 身份 | `qiankui-github` |
| 区域 | Resource Group | `qiankui-japaneast` |
| 区域 | Virtual Network | `qiankui-vnet-japaneast` |
| 区域 | Infrastructure Subnet | `qiankui-snet-aca-japaneast` |
| 区域 | Container Apps Environment | `qiankui-japaneast` |
| 区域 | Container App | `qiankui-relay-japaneast` |
| 平台 | Managed Resource Group | `qiankui-infra-japaneast` |
| 平台 | Load Balancer / Public IP | Azure 所造之 `capp-svc-lb*` |

资源组不作嵌套。`qiankui-infra-<region>` 由对应的 Container Apps Environment 独占，勿手改其内资源。`NetworkWatcherRG` 为订阅级网络诊断资源，不属潜逵命名体系，亦不纳入本项目 IaC。

ACR、Key Vault 与两个用户分配身份只建一次。每一区域必须另有 VNet、子网、Environment、Container App、负载均衡器与公网 IP；区域资源不得跨地复用。

## 区域簿

诸区域尽录于 [`deploy/azure/regions.json`](../deploy/azure/regions.json)。日本东部初值如下：

```json
{
  "sharedLocation": "japaneast",
  "regions": [
    {
      "slug": "japaneast",
      "location": "japaneast",
      "networkAddressPrefix": "10.42.0.0/24",
      "infrastructureSubnetAddressPrefix": "10.42.0.0/27"
    }
  ]
}
```

`sharedLocation` 一经初立即应保持不变；它决定 ACR、Key Vault 与两个共享身份的所在地。新增区域只追加 `regions` 项，不改变共享层位置。

欲添美国东部，可续录一项，并分配不重叠网段：

```json
{
  "slug": "eastus",
  "location": "eastus",
  "networkAddressPrefix": "10.43.0.0/24",
  "infrastructureSubnetAddressPrefix": "10.43.0.0/27"
}
```

欧洲可依次用 `10.44.0.0/24`。网段虽未必立即互联，仍须全局不重叠，以免日后 VNet peering 或内网控制面受阻。

## 权界

- `qiankui-runtime` 可拉取共享 ACR 镜像。
- 运行身份只能读取 `relay-token` 与各区域的 `tls-cert-<region>`、`tls-key-<region>`。
- `tls-ca-cert`、`tls-ca-key` 共用；CA 私钥不授予 Container App。
- 每个区域使用按该区域 Azure FQDN 签发的服务端证书，不能跨区域复用。
- `qiankui-github` 对 ACR 只有 Push 权，对各区域资源组只有 Container Apps Contributor。
- GitHub 身份不能管理 Key Vault、VNet、Managed Environment 或托管身份，也没有 Key Vault 数据面权限。
- GitHub `production` 环境保存三个 Azure 标识型 secret；没有客户端密码。
- 本地符节只写入权限为 `0600` 的简牍，不入 Git。

## 初立

先登录 Azure 与 GitHub CLI：

```sh
az login
gh auth login
```

而后在仓库根目录运行：

```sh
deploy/azure/bootstrap.sh \
  --github-repository OWNER/REPOSITORY
```

此令依次建立共享层，构建一次 relay 镜像，再遍历区域簿：

1. 建 `qiankui-shared`、ACR、Key Vault 与两个身份。
2. 生成或复用随机 256 位符节与专用 CA。
3. 为每一区域建立区域 RG、VNet、子网及 Container Apps Environment。
4. 明定平台资源组为 `qiankui-infra-<region>`。
5. 为各区域 FQDN 签发独立服务端证书。
6. 建外部 TCP `8443` 的 Container App，并维持一个最小副本。

只建立新添的一个区域：

```sh
deploy/azure/bootstrap.sh \
  --github-repository OWNER/REPOSITORY \
  --region eastus
```

再将 OIDC 标识写入 GitHub `production` 环境：

```sh
deploy/azure/configure-github.sh \
  --github-repository OWNER/REPOSITORY
```

## 近端

安装近端命令并选择区域：

```sh
deploy/azure/configure-client.sh --region japaneast
```

此令把 `qiankui` 装入 `~/.cargo/bin`，从共享 Key Vault 取得 CA 与符节，再把所选区域的 relay 写入 `~/.config/qiankui/config.toml`。随后只需：

```sh
qiankui
curl --proxy socks5h://127.0.0.1:1080 https://example.com
```

## 发版

每次推送 `main` 后：

1. CI 运行格式、Clippy、测试、release 构建、shell 语法、区域簿校验及 Bicep 编译。
2. CI 成功方触发 Azure 部署。
3. GitHub 以 OIDC 换取短期 Azure 令牌。
4. BuildKit 以完整 Git SHA 为标签构建一次镜像并推入共享 ACR。
5. 流水线从区域簿生成矩阵，并行更新各区域 Container App。
6. 每一区域皆须达到 `Healthy`、`Running`，流水线方为成功。

流水线不使用 `latest` 标签，亦不读取 relay 符节或 TLS 私钥。

## TLS 与符节

Container Apps 的 TCP ingress 原样转发字节，由 `qiankui-relay` 自行终止 TLS。Azure 不准外部 TCP ingress 使用 `80` 或 `443`，故公网端口固定为 `8443`。

初版使用一枚共享专用 CA，为各区域 Container App 的 Azure FQDN 分别签证。轮换全网符节：

```sh
deploy/azure/rotate-token.sh
deploy/azure/configure-client.sh --region japaneast
```

仅重启一区：

```sh
deploy/azure/rotate-token.sh --region japaneast
```

## 察看

```sh
az containerapp show \
  --resource-group qiankui-japaneast \
  --name qiankui-relay-japaneast \
  --output table

az containerapp revision list \
  --resource-group qiankui-japaneast \
  --name qiankui-relay-japaneast \
  --output table
```

## 费用

共享层主要产生 ACR Basic 与少量 Key Vault 操作费用；每增一区域，主要增加 Container Apps 常驻最小副本、区域公网 IP、负载均衡与网络出口费用。资源组本身不收费。宜在 Azure Cost Management 设预算与告警。
