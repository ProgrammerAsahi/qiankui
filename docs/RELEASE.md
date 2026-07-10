# 发章之法

近端 `qiankui` 以 GitHub Release 为章府，以 Minisign 为署，以 Homebrew tap 为常用取径。远端 `qiankui-relay` 仍由 Azure 流水线独立部署，二者不可混包。

## 一章之物

标签 `vX.Y.Z` 所发者有：

- `qiankui-aarch64-apple-darwin`：供内置更新器直接取用。
- `qiankui-vX.Y.Z-aarch64-apple-darwin.tar.gz`：供 Homebrew 取用。
- `qiankui-update.json`：稳定章、目标、下载地址、字节数与 SHA-256。
- `qiankui-update.json.minisig`：前项清单之 Minisign 署名。
- `SHA256SUMS`：供人工复核诸物。

今章仅发 macOS arm64。公钥在 `packaging/qiankui-release.pub`；私钥永不得入库。

## GitHub 之备

仓库设 `release` Environment，并藏四项 Environment Secret：

```text
MINISIGN_SECRET_KEY
MINISIGN_PASSWORD
HOMEBREW_TAP_DEPLOY_KEY
HOMEBREW_TAP_KEY_PASSWORD
```

此 Environment 仅许 `v*.*.*` 标签取用。前二者署更新清单；后二者惟许写 `ProgrammerAsahi/homebrew-qiankui`。签名私钥宜加密离库备份，密码宜另存密码库。若失签名私钥，不可径易公钥；须先由旧钥所署版本引入新钥，再行轮换。

## 发章

先使 `Cargo.toml` 版本与欲发标签一致，并令主分支 CI 通过：

```sh
git tag -a v0.1.0 -m "qiankui v0.1.0"
git push origin v0.1.0
```

`Release Client` 流水线将复验版本、运行测试、在 GitHub arm64 macOS runner 构建、署清单、以草稿附齐诸物、发布不可变 Release，而后更新 Homebrew Formula。GitHub 又为整章及其资产自署证明；同版本既发，资产与标签皆不可改。若后段失利，重跑时只下载、验明既有章物，不覆写之。

## 用者更新

直接取 GitHub 客户端者可行：

```sh
qiankui update --check
qiankui update
```

程序先验清单署名，再验二进制长度、SHA-256 与自报版本，最后在原目录原子替换。Homebrew 所装之本仍由 Homebrew 掌管：

```sh
brew upgrade ProgrammerAsahi/qiankui/qiankui
```
