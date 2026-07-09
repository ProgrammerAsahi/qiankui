# Azure 置邮之法

此章在 Azure 日本东部（`japaneast`）建立潜逵置邮。运行面为 Azure Container Apps Consumption，入口为原始 TCP `8443`；镜像藏于 ACR，符节与 TLS 信证藏于 Key Vault。GitHub Actions 借 OIDC 短期登录，不设客户端密码。

## 资源

| 资源 | 名称 | 用途 |
|---|---|---|
| Resource Group | `qiankui` | 总辖诸资源 |
| Virtual Network / Subnet | `qiankui` | Container Apps 外部 TCP 所需 |
| Container Apps Environment | `qiankui` | 日本东部 Consumption 环境 |
| Container App | `qiankui` | 单副本 relay |
| Managed Identity | `qiankui` | 运行时读取三个 Key Vault secret、拉取镜像 |
| Managed Identity | `qiankui-github` | GitHub OIDC 部署身份 |
| ACR | `qiankui<订阅散列>` | 全局唯一的 relay 镜像仓库 |
| Key Vault | `qiankui-<订阅散列>` | 全局唯一的运行秘密仓库 |

ACR 与 Key Vault 名称须在全 Azure 唯一，故附只由订阅 ID 推得的稳定后缀；源码不保存订阅、租户或账户信息。

## 边界

- Container App 托管身份可拉取 ACR 镜像。
- 该身份仅能读取 `relay-token`、`tls-cert`、`tls-key` 三个具体 secret。
- CA 私钥虽存 Key Vault，Container App 无权读取。
- GitHub 部署身份仅有 Container Apps Contributor 与 ACR Push；不能管理 Key Vault、VNet 或托管身份，也没有 Key Vault 数据面权限。
- GitHub `production` 环境保存 `AZURE_CLIENT_ID`、`AZURE_TENANT_ID`、`AZURE_SUBSCRIPTION_ID` 三个标识型 secret，以及非敏感变量 `AZURE_ACR_NAME`；没有客户端密码。
- `production` 环境的部署分支策略只允许 `main`，与 OIDC 的 environment subject 合为两重边界。
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

此令依次：

1. 建日本东部资源组、VNet、Container Apps 环境、ACR、Key Vault 与托管身份。
2. 由 ACR 云端构建 relay 镜像，本机不必有 Docker。
3. 生成随机 256 位符节及专用 CA、服务端证书，写入 Key Vault。
4. 以逐 secret RBAC 授权运行身份。
5. 建立外部 TCP `8443` 的 Container App，并维持一个最小副本。

再将 OIDC 标识写入 GitHub `production` 环境：

```sh
deploy/azure/configure-github.sh \
  --github-repository OWNER/REPOSITORY
```

最后装近端命令，并从 Key Vault 安全取得公证与符节：

```sh
deploy/azure/configure-client.sh
```

此令把 `qiankui` 装入 `~/.cargo/bin`，写好 `~/.config/qiankui/config.toml` 与 `azure-ca.pem`。随后只需：

```sh
qiankui
curl --proxy socks5h://127.0.0.1:1080 https://example.com
```

## 发版

每次推送 `main` 后：

1. `.github/workflows/ci.yml` 运行格式、Clippy、测试、release 构建、shell 语法与 Bicep 编译。
2. CI 成功方触发 `.github/workflows/deploy-azure.yml`。
3. GitHub 以 OIDC 换取短期 Azure 令牌。
4. BuildKit 构建镜像，以完整 Git SHA 为标签推入 ACR。
5. Container Apps API 只更新不可变镜像并发布新 revision，再等候其 `Healthy`、`Running`。

流水线不使用 `latest` 标签，亦不读取 relay 符节或 TLS 私钥。

## TLS

Container Apps 的 TCP ingress 原样转发字节，由 `qiankui-relay` 自行终止 TLS。Azure 不准外部 TCP ingress 使用 `80` 或 `443`，故公网端口固定为 `8443`。

初版使用专用 CA 为 Container App 的 Azure FQDN 签证；公证下发本机，私钥仅在 Key Vault。日后绑定自有域名时，可改接 ACME 自动续期，而无需改变 relay 协议。

## 察看

```sh
az containerapp show --resource-group qiankui --name qiankui --output table
az containerapp revision list --resource-group qiankui --name qiankui --output table
az containerapp logs show --resource-group qiankui --name qiankui --type system --follow
```

轮换符节后应立即重配近端：

```sh
deploy/azure/rotate-token.sh
deploy/azure/configure-client.sh
```

查看 Key Vault secret 时勿用 `--query value`，除非确需在受控本机重配客户端。Bicep 初立时采用无版本 Key Vault URI；`rotate-token.sh` 则显式绑定新版本并等待 revision 健康重启，以免受后台同步时延所累。

## 费用

此部署会产生 Azure 费用，主要来自 ACR Basic、Container Apps 常驻最小副本、网络出口及少量 Key Vault 操作。VNet 本身通常不单独计费。请在 Azure Cost Management 设预算与告警。
