# MijiaLAN for fnOS · 0.1.1 开发测试版

面向 Intel N150 等 x86_64 飞牛设备的原生 FPK，不依赖 Docker、Node.js 或 Python。Rust 后台二进制内嵌 Web 管理界面，网页通过飞牛统一网关和 Unix Socket 打开。

## 安装与首次使用

1. 在飞牛应用中心手动安装 `MijiaLAN-fnOS-0.1.1-x86_64-preview.fpk`。
2. 安装向导默认管理密码为 `12345`，可以修改。登录网页后也可以通过右上角“修改密码”更改，立即生效且重启保留。
3. 默认仅使用飞牛网关打开管理页面。需要从局域网脚本直接调用 API 时，在安装向导启用 8787 端口；此时接口地址为 `http://飞牛IP:8787/api/...`。
4. 在“米家账号”中扫码登录并等待设备同步，可按家庭和房间筛选。设备卡片支持折叠；勾选“接入 HomeKit”后弹出当前设备的独立二维码，每台设备单独配对，互不带入。旧版共享桥需先从苹果家庭中移除，再逐台扫码。
5. 添加自定义设备时可选“电脑：WOL + Ping”“HTTP API 开关”“主动上报传感器”。手机窄屏使用纵向布局。

## 实现范围

- 复用上游 Rust 米家协议、规格、旧协议本地适配及 HomeKit 加密/配对模块。
- 米家账号设备清单同步，保留没有本地 IP/Token 的云设备；每 5 分钟自动同步并提供手动同步。
- 按型号读取 MIoT 规格，自动生成属性控制；不按品牌限制。支持本地读取时优先本地，失败后走米家云，定期重试本地。
- 云端 MIoT 属性读写与动作通道；同步或请求失败会显示错误。失败同步不会清空旧清单。
- WOL、Ping 连续失败判断、开机等待；HTTP GET/POST/PUT/PATCH/DELETE/HEAD 请求、请求头、JSON 请求体、JSON Pointer 状态读取和数值/布尔映射。
- 自定义 HomeKit 开关、插座、温度/湿度传感器；主动状态上报接口；自定义设备增删改。
- 设备控制与状态确认分开；默认以读取/上报为准，只有显式选择 optimistic 模式才按动作推定状态。
- 账号会话、HTTP 凭证、HomeKit 身份和自定义配置加密保存。服务主密钥保存在应用私有数据目录的 0600 文件中，无桌面钥匙串依赖。
- 安装、启停、升级脚本和卸载保留/清除数据选项。

## 验证情况与边界

已完成：Mac 上 Rust 自动测试；本机回环 HTTP 控制与状态读回、WOL UDP 数据包、Ping、主动上报、认证与参数校验、加密持久化、修改密码后旧令牌失效与重启保留、删除设备的端到端测试；自定义设备 HomeKit 服务类型和稳定 ID 测试；手机 390×844 布局检查；Linux x86_64 交叉编译；官方 fnpack 1.2.3 打包校验。

**已在电脑端读取真实米家账号设备，并收到部分设备配对反馈；尚未完成飞牛真机安装、全型号兼容性和 iPhone 完整配对验收。此包是测试候选，不是已验收的稳定版。**

- 不能承诺米家所有型号的全部功能均兼容。没有公开 MIoT 属性或需要私有旧协议的设备会显示缺少规格/控制错误，需根据实际型号补适配。米家 App 能控制并不代表其第三方云接口开放了相同能力。
- 云端状态目前按约 30 秒轮询，不是实时推送；自动清单同步逐设备读取规格，大量未知型号初次同步可能较慢。
- 同步成功后替换账号设备清单；退出账号立即移除米家设备，保留自定义设备，并丢弃旧同步任务的结果。切换账号清除旧云端缓存。专门的云端旧 miIO RPC 适配和完整设备事件订阅尚未实现。
- 当前只提供开关/插座及温湿度自定义类型；复杂多属性 HTTP 设备、门磁/运动事件、任意脚本执行没有实现。
- HTTP 请求体当前为 JSON，不支持 Python 代码、任意 shell 或 requests 脚本执行。
- Ping 需要飞牛系统的 `/bin/ping` 可执行且应用用户有 ICMP 权限；Ping 是网络可达性，不等同于电脑电源状态。WOL 需要电脑 BIOS/网卡支持，关机需另配 API。
- 飞牛统一网关、应用用户权限、手机 App 内嵌页面和卸载数据保留行为需在实际 fnOS 版本确认。
- 同一设备已确认本地可用时，写入失败不会立即再向云端重发，以免超时导致重复动作。

## API 示例

请求均要求 `Authorization: Bearer <管理密码>`。通过飞牛统一网关访问还需飞牛登录会话；安装时开启的独立 API 端口只要求应用密码。

```http
POST /api/custom
Content-Type: application/json
Authorization: Bearer <管理密码>

{
  "id": "desk-power",
  "name": "书桌电源",
  "kind": "switch",
  "bridged": true,
  "interval_secs": 10,
  "on": {"type":"http","request":{"url":"http://192.168.1.50/api/power","method":"POST","headers":{},"body":{"on":true}}},
  "off": {"type":"http","request":{"url":"http://192.168.1.50/api/power","method":"POST","body":{"on":false}}},
  "probe": {"type":"http","request":{"url":"http://192.168.1.50/api/status","method":"GET"},"pointer":"/data/power"}
}
```

状态字段直接返回布尔值。若返回字符串 `"on"`，在 probe 中增加 `"on_value":"on"`。温度/湿度返回数值。

主动上报示例：

```json
{"id":"room-temperature","name":"室温","kind":"temperature","bridged":true,"probe":{"type":"report"}}
```

创建后向 `POST /api/custom/report` 发送 `{"id":"room-temperature","value":23.5}`。

常用接口：

| 接口 | 用途 |
| --- | --- |
| GET /api/status | 账号、同步、桥状态 |
| GET /api/devices | 统一设备状态 |
| GET /api/custom | 自定义配置，含认证配置，仅管理员可读 |
| POST /api/custom | 添加或更新自定义设备 |
| POST /api/custom/delete | `{"id":"..."}` |
| POST /api/custom/report | 主动上报模式的设备状态 |
| POST /api/device/write | `{"did":"custom:desk-power","key":"2.1","value":true}` |
| POST /api/device/refresh | `{"did":"..."}` |
| POST /api/device/bridge | `{"did":"...","enabled":true}` |
| POST /api/xiaomi/login | `{"region":"cn"}` 获取扫码二维码 |
| POST /api/xiaomi/sync | 手动同步 |
| POST /api/xiaomi/logout | 清除本地云会话 |
| POST /api/device/state | `{"did":"..."}` 读取缓存状态 |
| POST /api/password | current_password、new_password、confirm_password |

## 构建与开发

Rust 1.90+，测试构建使用 1.99.1。默认 workspace 是无桌面的 daemon，原桌面源码保留但不参与构建。

```bash
cargo test -p mijialan-core -p mijialan-homekit -p mijialan-daemon
cargo build -p mijialan-daemon
MIJIALAN_DATA_DIR=/绝对路径/测试数据 MIJIALAN_LISTEN=127.0.0.1:8787 cargo run -p mijialan-daemon
python3 scripts/test-daemon.py target/debug/mijialan-daemon
```

使用 `scripts/build-fnos.sh` 构建 FPK。Mac 交叉构建需要 cargo-zigbuild、Zig 和 `x86_64-unknown-linux-gnu` Rust target；设置 `FN_PACK` 为官方 fnpack 可执行文件路径。Linux 原生构建需要系统 glibc 不高于目标设备版本。此候选包以 glibc 2.31 为目标。

上游：[iPocket-APP/MijiaLAN 的 MijiaLAN-rs 分支](https://github.com/iPocket-APP/MijiaLAN/tree/MijiaLAN-rs)。飞牛包为本地衍生测试构建，保留上游许可证及第三方声明。

## 当前电脑端验收

- 家庭数量不写死，使用米家返回的家庭 ID 区分；已覆盖 7 个同名家庭、同名房间和家庭设备分页的模拟测试。重名家庭在筛选项中附带 ID。
- HTTP 自定义设备直连目标接口，避免系统代理干扰局域网设备。
- RTSP 摄像头已集成：自定义设备 → RTSP 摄像头。支持 H.264 实时视频、网页实时预览、独立配对/重置/停用；暂不提供向 HomeKit 的 H.265 转码、录像或对讲，也不能通过米家扫码自动取得摄像头视频地址。
- 当前提供 0.1.1 FPK 测试候选，用于飞牛真机验收。

- 每台设备使用独立 HomeKit 身份与端口；重启保留配对，停用和重置只影响选中的设备。已用真实 TCP 配对协议测试验证设备隔离，iPhone 实际配对仍待验收。

## RTSP 摄像头

视频使用原生 go2rtc 1.9.15 与 FFmpeg 子进程，无需 Docker。通过 `MIJIALAN_CAMERA_BIN` 和 `MIJIALAN_FFMPEG_BIN` 指定可执行文件；默认使用 daemon 同目录的 `go2rtc` 和 `ffmpeg`。缺少组件时界面会提示，仍可保存摄像头配置。

在“自定义设备”点击“＋ RTSP 摄像头”，填写名称和 RTSP 地址。先点击“实时预览”验证视频读取，再“启用并配对”。每台摄像头有独立二维码；配对成功后在苹果家庭中看实时画面。设备与手机需在网络可互通且允许 mDNS 的局域网。

摄像头配置及原生组件配对文件保存在数据目录 `cameras/`（目录 0700、配置 0600）；其中包含 RTSP 凭据，原生组件运行需要读取这些文件。管理列表不返回完整 RTSP 地址，编辑时留空保留原地址。摄像头只开放 HomeKit 配对入口，组件的管理 API、流列表和网页不开放到局域网。

已完成本机自动测试：生成 H.264 视频 → RTSP → HomeKit 配对 → SRTP → 解码成画面，以及双摄像头身份隔离、编辑与重启保留配对、独立停用/重置/删除。测试命令：

```sh
python3 scripts/test-camera.py /path/to/mijialan-daemon /path/to/go2rtc /path/to/ffmpeg
```

尚需用户实际摄像头、iPhone 家庭 App 和飞牛真机验收。FPK 包含 Linux x86_64 视频组件与对应许可证；发布时请同时上传视频组件源码包。构建时设置 CAMERA_RUNTIME_DIR 指向包含 bin/ 与 licenses/ 的组件目录。

参考：[go2rtc HomeKit](https://github.com/AlexxIT/go2rtc/blob/v1.9.15/internal/homekit/README.md)。go2rtc 使用 MIT 许可证；FFmpeg 的分发许可证以实际构建配置为准。

## 0.1.1 映射与状态边界

- 未识别的属性、枚举和动作不再自动变成 HomeKit 开关。复杂设备可能只有部分功能出现在苹果家庭，其他功能保留在管理页面。
- 晾衣架 zjjsw.airer.sa006 的照明映射为灯、风干映射为风扇；没有高度反馈，不伪造升降位置。烘干、杀菌保留网页控制。
- 米家落地扇 dmaker.fan.p8 支持三档调速（对应约 33%、67%、100%）、开关、摇头和童锁；睡眠风、定时等仍留在网页。
- 欧普浴霸 opple.bhf_light.wlyb 分别映射照明、小夜灯、换气、吹风、取暖。取暖只允许制热，目标温度 32–42℃；风速、摆风、干燥、智能沐浴仍留在网页，避免未经确认的关联控制。其他型号仍需逐型号完善。
- 控制后显示“正在确认”，优先读回状态；HTTP 状态读取不再被常规轮询缓存阻挡。云端仍存在网络延迟。
- 手机配对超时尚未在所有真实环境解决。手机与 NAS 需允许 mDNS 以及各设备动态 HomeKit TCP/UDP 端口。
- 默认管理密码为 `12345`，首次使用请修改。安装包不带开发者账号和设备数据。
