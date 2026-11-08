# “Accessory out of compliance” 检查

日期：2026-11-04。

用户截图显示 Apple 家庭拒绝添加配件，并报告 `Accessory out of compliance`。仅凭此提示不能确定具体哪个服务、特征或协议步骤失败。用户说明无法取得反馈者的版本、型号等信息后，本次继续通过异常设备规格、模拟 miIO 设备及真实加密 HAP 连接构造复现，不以补充这些信息作为继续排查的前提。

## 原版已知问题与 Rust 的区别

原 Swift 项目的 `docs/homekit-pairing-failure-2026-10-31.md` 记录过同一提示：空调制冷阈值的类型写成 `0D`，iPhone 在读取数据库时拒绝了前导零。那次故障有手机日志及修复后的真实配对成功证据，但不能据此认定这张新截图仍由相同问题造成。

Rust 的空调映射已经使用 `D`。本次新增数据库测试独立检查所有服务与特征类型的规范十六进制表示，并明确检查空调包含 `D`，没有复现此前的 `0D` 问题。[Apple 的 UUID 编码实现](https://github.com/apple/HomeKitADK/blob/master/HAP/HAPUUID.c)使用无前导零的短形式。

进一步只读核对了[公开发行列表](https://github.com/iPocket-APP/MijiaLAN/releases)及 [`v0.1.0` 标签下的原版映射代码](https://github.com/iPocket-APP/MijiaLAN/blob/v0.1.0/Sources/MijiaHomeKitBridge/HomeKitAccessoryMapper.swift#L138)：该正式版的制冷阈值常量也已是 `D`。因此，如果反馈者使用的是这份正式版，旧的 `0D` 根因就不适用；不能把未知版本的新反馈直接归为已知旧故障。

## 确认并修复的三项兼容风险

| 项目 | 修复前 | 修复后 |
| --- | --- | --- |
| 主桥协议信息 | AID 1 只有 Accessory Information，缺少 HAP Protocol Information | 仅在主桥补齐服务 `A2` 与只读特征 `37`，版本 `1.1.0`；原有桥及设备的 AID/IID 保留 |
| Configured Name | `E3` 只有 `pr`、`pw`；纯订阅请求会被误判为无效改名，事件读取也只支持设备绑定 | 增加 `ev` 并实现纯订阅、改名后的及时数据库刷新、读回及事件通知；未声明事件权限的特征返回 `-70406` |
| 字符串长度 | 生成的名称没有字节上限，改名按 64 个字符而非 UTF-8 字节检查 | 输出名称、型号及序列号在 UTF-8 字符边界限制为默认 64 字节；改名按 64 字节验证，原始设备清单及已有用户名称不被截断或覆盖 |

主桥协议信息的要求见 [Apple 服务定义](https://github.com/apple/HomeKitADK/blob/master/HAP/HAPServiceTypes.h)，[HAP-NodeJS 主配件发布流程](https://github.com/homebridge/HAP-NodeJS/blob/latest/src/lib/Accessory.ts)也会补齐此服务。Configured Name 的权限可对照 [HAP-NodeJS 特征定义](https://github.com/homebridge/HAP-NodeJS/blob/latest/src/lib/definitions/CharacteristicDefinitions.ts)。默认字符串长度来自 [Apple 的 IP 配件序列化实现](https://github.com/apple/HomeKitADK/blob/master/HAP/HAPIPAccessory.c)。

这三项是已确认的实现缺口及修复，尚未在反馈者的 iPhone 上证明它们与截图故障之间的因果关系。

## 首轮验证

- 新增 4 项数据库回归：规范 UUID、AID/IID 唯一性、已映射服务的必需特征、权限枚举、值类型及声明范围；主桥协议服务；名称通知权限；超长中文与 emoji 名称的 UTF-8 字节边界。覆盖全部 10 个内置型号、4 台在线演示设备及 11 台控制中心演示设备。
- 修复前：3 项测试失败，分别明确报告缺少主桥协议服务、Configured Name 缺少 `ev`、名称超过 64 字节；内置型号的类型与必需特征检查通过。修复后 4 项全部通过。
- 新增 1 项独立控制器的真实 TCP 回环测试：完成 Pair Setup / Pair Verify 后读取加密数据库，检查协议版本，订阅名称并收到初始事件；66 字节中文名称返回 `-70410`，64 字节名称可写入、读回并收到变更事件；只读 Name 的事件订阅返回 `-70406`，桥配对身份保留。
- `cargo test --locked -p mijialan-homekit`：9 项全部通过。
- `cargo fmt --all --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`：通过。

本次修改和验证位于 Rust 源码及隔离测试中，未替换正在运行的正常客户端，未重置真实家庭配对或写入真实米家设备。没有把独立控制器测试等同于反馈者的 Apple 家庭实机验收；其具体版本和设备组合仍需补充信息后核对。

## 深入复现与追加修复

首轮正常型号与正常读数的检查不足以覆盖动态规格及设备异常。追加测试在修复前同时复现了以下六项问题：

| 复现场景 | 修复前行为 | 修复后的行为 |
| --- | --- | --- |
| 设备返回错误类型的电源值、亮度 255、异常色温等 | 值直接进入 `/accessories`，可能与声明的 bool、整数或范围不符；普通读取与事件也未校验 | 建立统一的 HAP 数值校验，覆盖初始数据库、实时数据库、读取及事件。不可用或不合法读数在数据库中为 `null`，普通读取和事件返回 `-70402`，配件及服务保留 |
| 窗帘原始位置范围 0–255、读数 73 | 初始百分比始终为 0；实时百分比可能带小数，与 uint8 声明不符 | 初始和实时值共用相同转换；按声明的步长及整数格式得到 29%，并验证原始范围，避免除零或非法范围进入响应 |
| 加湿器/除湿器没有可写目标湿度 | 仍暴露 Humidifier Dehumidifier 服务，却缺少该模式条件必需的湿度阈值 | 只有可安全写入目标湿度时才生成原生服务，否则保留通用开关和传感器；不虚构设备没有的阈值 |
| 仅制热或仅制冷设备 | Target State 的完整 0–2 范围会声明未实现的工作模式 | 使用排序且去重的 `valid-values` 明确列出实际支持值，固定模式同样限制范围；相应的制热、制冷或湿度阈值按支持模式提供 |
| 规格重复声明同一个属性 | 同一服务可能生成重复的特征 IID，服务自身也可能重复 | 按稳定服务键复用服务，按同一服务内的特征类型去重；保持原有身份和已分配标识 |
| 一个属性有 150 个选项 | 每个选项生成一个开关，设备共生成 152 个服务 | 优先保留主功能及传感器，限制单设备为 100 个服务；超出部分不建立隐藏绑定，并通过家庭桥信息明确提示附加功能受限；客户端原有能力不删减 |

`null` 处理遵循 [Apple 的配件数据库序列化实现](https://github.com/apple/HomeKitADK/blob/master/HAP/HAPIPAccessory.c#L861)。加湿器/除湿器的条件阈值及空调的条件温度要求可对照 [Apple 服务定义](https://github.com/apple/HomeKitADK/blob/master/HAP/HAPServiceTypes.h#L805)。UInt8 `valid-values` 的升序、唯一约束见 [Apple 校验实现](https://github.com/apple/HomeKitADK/blob/master/HAP/HAPAccessoryValidation.c#L548)。100 服务上限与 [HAP-NodeJS 配件实现](https://github.com/homebridge/HAP-NodeJS/blob/latest/src/lib/Accessory.ts#L77)一致。

另外，HAP 写入请求在转换到设备单位前统一检查声明的类型、范围、步长和允许值：例如小数亮度 `2.5`、超出声明范围的 `10000` mired 会返回 `-70410`，不再先取整或钳制后执行另一条有效设备命令。

### 深入验证的实际链路

- 6 项映射回归修复前全部失败、修复后全部通过；另有 2 项输入输出数值约束回归。
- 增加一个仅监听 `127.0.0.1` 的真实 miIO UDP 模拟净化器，使用内存凭据，经共享 Engine 进行实际握手和加密读取；随后使用独立 HAP 控制器完成 TCP 配对及加密读取。
- 当 UDP 设备返回湿度 `123`、温度 `-3276.8`、电源字符串 `"on"` 时，`GET /accessories` 仍返回 200，相关不可用值为 `null`；`GET /characteristics` 和订阅事件返回 `-70402`。恢复为湿度 55 后，不重置桥即收到有效事件。整个测试没有向 UDP 设备发出控制写入，桥身份及配对保留。
- 经真实加密 TCP 验证，非法小数亮度和超范围色温均返回 `-70410`，内存设备状态保持原值。
- 增加配对回滚测试：控制器移除自己的管理员配对后，桥恢复可配对状态；另一个控制器使用原桥身份和 PIN 完成新的 Pair Setup / Pair Verify，并成功读取数据库。未复现这一回滚路径卡住身份的问题。
- 当前 `cargo test --locked -p mijialan-homekit`：**19 项通过**，其中映射/数值 8 项、数据库 4 项、加密传输/配对 7 项。`cargo fmt --all --check`、全工作区全目标 Clippy 严格检查和 Mac 桌面构建均通过。

原 Swift 客户端和真实 Rust 客户端进程未被替换或重启。本轮源码修复没有安装到正常运行包，也没有进行新的 iPhone 家庭实机配对。能够确认上述 Rust 代码缺陷及修复效果，尚不能确认截图用户遇到的是其中哪一项。
