# 潜逵部署之法

本文分“近端”与“置邮”二处。近端在使用者电脑，置邮在自有或获准使用之 Linux 服务器。项目不指定云商，普通 VPS、云主机或自管虚拟机皆可。

## 一、职责分界

| 所在 | 二进制 | 职责 | 长期开放端口 |
|---|---|---|---|
| 本机 | `qiankui` | 监听本机 SOCKS5，将 TCP 请求送往置邮 | 默认仅 `127.0.0.1:1080` |
| 服务器 | `qiankui-relay` | 终止 TLS、验证符节、连接获准目标 | 建议公网 TCP 443 |
| 开发机/CI | Cargo 与源码 | 构建、测试 | 无 |

正式部署后，服务器不需要 Node.js，也不需要保存项目源码；若上传已构建的 Linux 二进制，亦不需要 Rust。

## 二、服务器条件

- 一台自有或明确获准使用的 Linux 主机。
- 一个解析至该主机的域名，例如 `relay.example.com`。
- 与域名匹配、由可信 CA 签发的证书和私钥。
- 防火墙或安全组允许客户端访问所选 TCP 端口。
- 至少一个非匿名、足够随机的符节。

证书续期后须重启 `qiankui-relay`，方能载入新证书。可将此动作接入既有 ACME 客户端的 deploy hook。

## 三、构建服务器二进制

macOS 二进制不可直接运行于 Linux。最简之法是在与目标相同架构的 Linux 环境构建：

```sh
git clone https://github.com/ProgrammerAsahi/qiankui.git
cd qiankui
cargo build --release --locked --bin qiankui-relay
```

构建完成后，安装二进制：

```sh
sudo install -m 0755 target/release/qiankui-relay /usr/local/bin/qiankui-relay
```

也可在 Linux CI 构建后，仅把 `qiankui-relay` 成品传至服务器。发布前应核对成品哈希与构建来源。

## 四、建立运行身份

```sh
sudo useradd --system --home /nonexistent --shell /usr/sbin/nologin qiankui
sudo install -d -m 0750 -o root -g qiankui /etc/qiankui
```

将证书与私钥置于：

```text
/etc/qiankui/cert.pem
/etc/qiankui/key.pem
```

私钥应仅准 root 与 `qiankui` 组读取：

```sh
sudo chown root:qiankui /etc/qiankui/cert.pem /etc/qiankui/key.pem
sudo chmod 0640 /etc/qiankui/cert.pem /etc/qiankui/key.pem
```

生成符节：

```sh
openssl rand -hex 32
```

以所得值建立 `/etc/qiankui/qiankui.env`：

```text
QIANKUI_TOKEN=将随机符节置于此
```

并限制权限：

```sh
sudo chown root:qiankui /etc/qiankui/qiankui.env
sudo chmod 0640 /etc/qiankui/qiankui.env
```

## 五、启用 systemd

项目附有 [`deploy/qiankui-relay.service`](../deploy/qiankui-relay.service)：

```sh
sudo install -m 0644 deploy/qiankui-relay.service /etc/systemd/system/qiankui-relay.service
sudo systemctl daemon-reload
sudo systemctl enable --now qiankui-relay
sudo systemctl status qiankui-relay
```

查阅日志：

```sh
sudo journalctl -u qiankui-relay
```

服务样例监听 TCP 443，并仅允许目标端口 80、443。若改用 8443，可移除 systemd 单元中的 `CAP_NET_BIND_SERVICE` 两行。

## 六、本机运行

在本机源码目录构建：

```sh
cargo build --release --locked --bin qiankui
```

运行：

```sh
export QIANKUI_TOKEN='与服务器相同的符节'
./target/release/qiankui \
  --listen 127.0.0.1:1080 \
  --relay https://relay.example.com
```

应用程序选择 SOCKS5 代理 `127.0.0.1:1080`。命令行试验宜用 `socks5h://`，使域名交由置邮解析：

```sh
curl --proxy socks5h://127.0.0.1:1080 https://example.com
```

## 七、上线核对

- 置邮未启用 `--allow-private`。
- 符节不少于 16 字节，且不写入命令历史、仓库或进程参数。
- 本机 SOCKS5 仅监听回环地址。
- TLS 证书与域名匹配，近端未使用 `--insecure`。
- 服务器没有开放匿名中继，目标端口列表取最小集合。
- 云商安全组与主机防火墙只开放必要入口。
- 已验证证书续期后的重启流程。
- 已遵守主机所在地法律、服务商条款及网络所有者授权范围。
