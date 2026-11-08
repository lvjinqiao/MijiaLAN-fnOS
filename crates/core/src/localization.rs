use std::sync::atomic::{AtomicBool, Ordering};
static ENGLISH: AtomicBool = AtomicBool::new(false);
pub fn configure(language: &str) {
    let language = if language.is_empty() {
        sys_locale::get_locale().unwrap_or_else(|| "zh-CN".into())
    } else {
        language.into()
    };
    ENGLISH.store(!language.starts_with("zh"), Ordering::Relaxed);
}
pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}
pub fn text(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    if !english() {
        return value.into();
    }
    let translated = match value {
        "设备离线" => "Device offline",
        "暂无读数" => "No reading yet",
        "正在获取设备规格…" => "Loading device specification…",
        "设备未响应，请检查 IP、网络权限和设备电源" => {
            "No response. Check the IP address, network permissions and device power."
        }
        "设备响应校验失败，请检查 Token" => {
            "Response authentication failed. Check the device token."
        }
        "握手成功，但加密请求未响应；请检查 Token 或设备的局域网支持" => {
            "The device answered discovery but not encrypted requests. Check its token and LAN support."
        }
        "设备" => "Devices",
        "设置…" => "Settings…",
        "打开 MijiaLAN Rust" => "Open MijiaLAN Rust",
        "退出" => "Quit",
        "退出 MijiaLAN Rust" => "Quit MijiaLAN Rust",
        "添加设备…" => "Add device…",
        "刷新状态" => "Refresh devices",
        "MijiaLAN Rust · 局域网设备控制" => "MijiaLAN Rust · Local device control",
        "发现" => "Discover",
        "添加" => "Add",
        "扫码" => "Import",
        "家庭" => "Home",
        "取消导入" => "Cancel import",
        "导入已取消" => "Import cancelled",
        "正在导入设备…" => "Importing devices…",
        "语言设置已保存" => "Language saved",
        "Token 必须是 32 位十六进制字符" => {
            "The token must contain 32 hexadecimal characters"
        }
        "设备不存在" => "Device not found",
        "设备没有凭据，请重新导入" => {
            "Missing device credentials; import the device again"
        }
        "全部设备" => "All devices",
        "我的设备" => "My devices",
        "发现设备" => "Discover devices",
        "添加设备" => "Add device",
        "扫码导入" => "QR import",
        "家庭桥接" => "HomeKit bridge",
        "设置" => "Settings",
        "Rust · 局域网设备控制" => "Rust · Local device control",
        "演示模式" => "Demo mode",
        "演示设备 · 不发送网络指令" => "Demo devices · No network commands",
        "本机连接 · 凭据安全保存" => "Local connection · Secure credentials",
        "局域网直连" => "Local connection",
        "刷新" => "Refresh",
        "＋ 添加设备" => "＋ Add device",
        "设备控制通过局域网完成" => "Device commands stay on your local network",
        "把家里的设备连接起来" => "Connect your home",
        "用米家 App 扫码导入，或填写已有的设备 IP 和 Token。" => {
            "Import with the Mi Home app, or enter a device IP and token."
        }
        "扫码导入设备" => "Import devices with QR",
        "手动添加" => "Add manually",
        "设备已连接" => "Connected",
        "等待连接" => "Waiting for connection",
        "全屋" => "Home",
        "‹ 返回设备" => "‹ Back to devices",
        "正在处理…" => "Working…",
        "设备状态已刷新" => "Device states refreshed",
        "设备已确认指令" => "Device acknowledged the command",
        "请输入有效的 IPv4 地址" => "Enter a valid IPv4 address",
        "演示模式不访问真实局域网" => "Demo mode does not access your network",
        "正在广播发现设备…" => "Discovering devices…",
        "连接后验证设备身份，再将凭据保存到本机。" => {
            "Verify the device before saving credentials on this computer."
        }
        "设备名称" => "Device name",
        "设备 IP 地址" => "Device IP address",
        "设备 Token" => "Device token",
        "验证并添加设备" => "Verify and add device",
        "Token 可通过扫码导入获取。现代固件的局域网广播通常不会返回 Token。" => {
            "Use QR import to obtain tokens. Modern firmware rarely exposes tokens through discovery."
        }
        "广播查找 miIO 设备，并按设备 ID 更新已知 IP。" => {
            "Find miIO devices and update known IP addresses by device ID."
        }
        "搜索局域网" => "Search local network",
        "使用米家 App 确认登录，导入设备凭据后即丢弃云端登录会话。" => {
            "Confirm sign-in with Mi Home. The cloud session is discarded after importing device credentials."
        }
        "中国大陆" => "Mainland China",
        "获取登录二维码" => "Get login QR code",
        "等待手机确认…" => "Waiting for phone confirmation…",
        "扫码和导入需要访问小米服务；日常设备控制只使用局域网。" => {
            "QR import connects to Xiaomi services. Everyday control uses the local network."
        }
        "正在获取二维码…" => "Requesting QR code…",
        "用米家 App 扫描二维码，并在手机上确认" => {
            "Scan with Mi Home and confirm on your phone"
        }
        "请启动正式模式后扫码导入设备" => "Open normal mode to import real devices",
        "Apple 家庭桥接" => "Apple Home bridge",
        "让家庭 App、Siri 和家居中枢通过这台电脑控制设备。" => {
            "Let Apple Home, Siri and home hubs control devices through this computer."
        }
        "启用 HomeKit 桥" => "Enable HomeKit bridge",
        "桥接时阻止闲置休眠" => "Prevent idle sleep while bridging",
        "HomeKit 桥已启动" => "HomeKit bridge started",
        "HomeKit 桥已停止" => "HomeKit bridge stopped",
        "闲置休眠设置已保存" => "Idle sleep preference saved",
        "桥接选择已保存" => "Bridge selection saved",
        "在 iPhone「家庭」中添加配件并扫描二维码，确认添加未认证配件。" => {
            "Add an accessory in Apple Home, scan the QR code, and accept the uncertified accessory."
        }
        "桥运行时需要电脑保持开机和网络连接。关闭窗口后继续在托盘后台运行。" => {
            "The computer must remain on and connected. Closing the window keeps the app running in the tray."
        }
        "切换浅色 / 深色外观" => "Toggle light / dark appearance",
        "凭据保存在本项目独立的系统凭据存储中。扫码登录会话不持久化。" => {
            "Credentials are encrypted with a key in the system vault. Cloud sign-in sessions are never persisted."
        }
        "演示模式：只使用内存数据" => "Demo mode: in-memory data only",
        "添加设备 ⌘N / Ctrl+N；刷新 ⌘R / F5；设置 ⌘, / Ctrl+,。" => {
            "Add: ⌘N / Ctrl+N · Refresh: ⌘R / F5 · Settings: ⌘, / Ctrl+,"
        }
        "应用参数" => "Apply value",
        "移除设备" => "Remove device",
        "设备已移除" => "Device removed",
        "设备已确认动作" => "Device acknowledged the action",
        "移除设备也会从 HomeKit 桥中移除，家庭中的相关场景可能受影响。" => {
            "Removing a device also removes it from the HomeKit bridge and can affect Home scenes."
        }
        "开启" => "On",
        "关闭" => "Off",
        "跟随系统" => "System language",
        "简体中文" => "简体中文",
        "English" => "English",
        _ => {
            return value
                .replace(" 台设备", " devices")
                .replace(" 台在线", " online")
                .replace("已添加 ", "Added ")
                .replace("响应错误：", "Response error: ")
                .replace("网络错误：", "Network error: ");
        }
    };
    translated.into()
}
