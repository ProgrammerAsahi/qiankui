# 潜逵

> 潜逵傍通，殊途同归。

潜逵者，私网通流之小器也。近端为径，受 SOCKS5 之请；远端置邮，验其符节，而后通于所往。其间以 TLS 1.3 蔽护，以 HTTP `CONNECT` 通行。今制务简，惟行 TCP，不设中枢，不蓄名籍，不为公门。

此为 v0.1.0，悉以 Rust 成之。所求惟三：可起、可通、可易。所谓可易者，近端之径与远端之邮，不知传输之细；异日若易 HTTP/2、HTTP/3 或他途，惟更“殊途”一部而已。

## 名物

- **潜逵**：总名。郭璞《江赋》有“潜逵傍通”。
- **殊途**（`src/shutu`）：承载传输之法。今仅有 TLS 上之 HTTP `CONNECT`。
- **置邮**（`src/zhiyou.rs`）：远端中继。古者置邮传命，站站相续。
- **符节**（`src/fujie`）：所持之凭信，兼辖出口之禁限。
- **径**（`src/socks5.rs`）：近端 SOCKS5 入口。

## 两端

```text
本机应用
  │ SOCKS5
  ▼
qiankui                    本地运行
  │ TLS 1.3 + HTTP CONNECT
  ▼
qiankui-relay（置邮）       Azure Container Apps / 自有服务器运行
  │ TCP
  ▼
所往之服务
```

本机惟需 `qiankui`；远机惟需 `qiankui-relay`。二者可由同一源码分别编成，运行时皆不需 Cargo，亦不需 Rust 工具链。一条 SOCKS5 连接，今对应一条 TLS 连接；此法未能复用，然简而易察，足以验通全程。

置邮不系于某家云商。今备二法：其一为寻常 Linux 主机，其二为 Azure 多地之 Container App。后者共用 ACR、Key Vault 与身份，而每地自有网络、入口与置邮；以 GitHub OIDC 发版，不造久存之云端密码。详见 [部署之法](docs/DEPLOYMENT.md) 与 [Azure 置邮](docs/AZURE.md)。

## 所具

- Rust 1.97.0，惟构建与开发时方需
- OpenSSL，惟生成试用证书时方需
- 一台自有或获准使用之远端主机，或己有之 Azure 订阅，惟正式跨机使用时方需

取源码后先校验：

```sh
cargo test --all-targets
```

## 初试

先造试用证书：

```sh
make dev-cert
```

另开一窗，起置邮：

```sh
export QIANKUI_TOKEN='change-this-token-at-least-16-bytes'
cargo run --bin qiankui-relay -- \
  --listen 127.0.0.1:8443 \
  --cert var/dev-cert.pem \
  --key var/dev-key.pem \
  --ports 80,443
```

再开一窗，起近端之径：

```sh
export QIANKUI_TOKEN='change-this-token-at-least-16-bytes'
cargo run --bin qiankui -- \
  --listen 127.0.0.1:1080 \
  --relay https://127.0.0.1:8443 \
  --ca var/dev-cert.pem
```

乃试之：

```sh
curl --proxy socks5h://127.0.0.1:1080 https://example.com
```

若置邮在远机，当以公认 CA 所署之证书及域名代试用证书；近端则不必传 `--ca`。`--insecure` 仅供仓促联调，勿用于常行。

## 简牍

近端可立简牍，免每起皆列诸参数。初立时若不传 `--token`，则隐字问符节：

```sh
qiankui config init \
  --relay https://relay.example.com:8443 \
  --ca ~/.config/qiankui/ca.pem
```

察之而不泄符节：

```sh
qiankui config show
qiankui config path
```

既立，径行一令即可：

```sh
qiankui
```

简牍默认在 `~/.config/qiankui/config.toml`，其权为 `0600`；目录权为 `0700`。`--config` 或 `QIANKUI_CONFIG` 可易其所在。命令行参数与 `QIANKUI_TOKEN` 可暂覆简牍，而不改其文。

## 号令

近端：

```text
config init            新立简牍；缺符节时隐字问之
config show            示简牍而隐符节
config path            示简牍所在
run                    依简牍起径；省略子命令亦同
--config <toml>        易简牍所在
--listen <host:port>   所守之 SOCKS5 地址，默认为 127.0.0.1:1080
--relay <https-url>    置邮地址
--token <secret>       符节；宜改用简牍或 QIANKUI_TOKEN
--ca <pem>             自署 CA 或证书
--insecure             不验置邮证书，仅供试验
--connect-timeout <ms> 连接限时，默认为 10000
```

置邮：

```text
--listen <host:port>   所守地址，默认为 0.0.0.0:8443
--cert <pem>           TLS 证书
--key <pem>            TLS 私钥
--token <secret>       符节；亦可取自 QIANKUI_TOKEN
--ports <list>         所许目的端口，默认为 80,443
--allow-private        许往私网、回环与保留地址；默认严禁
--connect-timeout <ms> 出口连接限时，默认为 10000
```

## 构建

```sh
cargo build --release --locked
```

其成品在：

```text
target/release/qiankui
target/release/qiankui-relay
```

不同操作系统与处理器须各自构建。欲部署 Linux 服务器，宜在同架构 Linux 主机或 Linux CI 上构建 `qiankui-relay`，不可将 macOS 成品径投 Linux。

## 戒约

置邮必验符节，且默认仅许 80、443，并拒私网、回环、链路本地及保留地址，以免沦为无主之公门或内网探针。程序不记所往域名，日志惟录启止。

此器惟宜用于己有之机、己辖之网，或明获允准之试验。毋设匿名开放中继，毋以侵人，亦当自察所在之法令与云商条款。

## 校验

```sh
make check
make test
make release
```

端到端之试自造短期证书，依次起回声服务、置邮与 SOCKS5 入口，而验符节拒纳、字节往返及半闭之义。

## 许可

此器以 Apache License 2.0 授人用之、改之、传之。其文具载于 [`LICENSE`](LICENSE)。

## 后章

初章既通，宜依次为之：

1. 以 HTTP/2 `CONNECT` 替今之逐流 TLS。
2. 为殊途定稳固之能力表与错误语义。
3. 增短期凭证与自动证书轮换。
4. 研 HTTP/3/MASQUE 与 UDP，仍不用自造密码。
5. 再议 TUN、图形界面与多节点编排。
