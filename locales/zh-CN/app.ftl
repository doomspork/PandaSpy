### PandaSpy — 应用外壳字符串。
###
### 本文件由应用的两侧共同使用：fluent-rs 用它构建托盘菜单和系统通知，
### @fluent/bundle 则用同一份文件构建窗口界面。不存在需要另行同步的第二份副本。
###
### en-US 是参考语言。若任何其他语言缺少此处定义的键（或定义了此处没有的键），
### `cargo xtask locale-check` 都会失败。

## Branding

# 产品名称。专有名词——在每种语言中都保留为 "PandaSpy"。它被定义为 term 而非
# message，以便译文能够变化其周围的措辞，而名称本身无需重新键入。
-brand-name = PandaSpy

## Application shell

window-title = { -brand-name }

## Tray menu

tray-show = 显示 { -brand-name }
tray-quit = 退出

## Window navigation

nav-add-printer = 添加打印机
nav-settings = 设置
nav-back = 返回

## Common

common-cancel = 取消

## Printer list

printer-list-empty-title = 暂无打印机
printer-list-empty-body = 添加一台打印机，即可在此处监控它。
printer-list-empty-cta = 添加打印机

## Connection status
##
## `status`/`reason` 直接来自客户端的会话状态机（`pandaspy-client`）；
## `*[unknown]` 涵盖此版本尚未识别的任何值，与线路解析器本身遵循的
## “绝不拒绝，只呈现所发生的情况” 规则一致。

connection-status = { $status ->
    [disconnected] 已断开
    [connecting] 连接中…
    [handshaking] 握手中…
    [connected] 已连接
    [failed] 连接失败
   *[unknown] 未知
}

connection-reason = { $reason ->
    [wrong-access-code] 访问码错误
    [unreachable] 无法连接打印机
    [tls] TLS 握手失败
    [certificate-changed] 证书已更改
    [protocol] 打印机返回异常响应
    [connection-closed] 连接已关闭
   *[unknown] 未知错误
}

## Printer card

print-status = { $status ->
    [idle] 空闲
    [preparing] 准备中
    [printing] 打印中
    [paused] 已暂停
    [finished] 已完成
    [failed] 失败
   *[unknown] 未知
}

card-remaining = 剩余 { $time }
card-progress-percent = { $percent }%
card-layer = 第 { $layer } / { $total } 层

card-nozzle-temp = { $hasTarget ->
    [yes] 喷嘴 { $current }° / { $target }°
   *[no] 喷嘴 { $current }°
}

card-bed-temp = { $hasTarget ->
    [yes] 热床 { $current }° / { $target }°
   *[no] 热床 { $current }°
}

card-chamber-temp = 腔体 { $current }°
card-task-name = 任务：{ $name }
card-print-error = 打印错误：{ $message }

card-expand = 显示详情
card-collapse = 隐藏详情
card-remove-label = 移除打印机
card-remove-confirm-title = 移除 { $name }？
card-remove-confirm-body = PandaSpy 将停止监控这台打印机，并忘记为其保存的访问码。
card-remove-confirm-confirm = 移除

## AMS

ams-kind = { $kind ->
    [standard] AMS
    [lite] AMS Lite
    [pro2] AMS 2 Pro
    [ht] AMS HT
   *[unknown] AMS
}

ams-humidity = 湿度 { $percent }%
ams-temperature = { $temp }°
ams-tray-empty = 空
ams-tray-remaining = 剩余 { $percent }%
ams-active-badge = 供料中

## HMS / health errors

hms-severity = { $severity ->
    [fatal] 致命
    [serious] 严重
    [common] 警告
    [info] 信息
   *[unknown] 提示
}

hms-code-only = 代码 { $code }
hms-learn-more = 了解更多

## Add printer

add-printer-title = 添加打印机
add-printer-tab-discovered = 已发现
add-printer-tab-manual = 手动
add-printer-tab-studio = Bambu Studio

add-printer-scanning = 扫描中…
add-printer-rescan = 重新扫描
add-printer-already-added = 已添加
add-printer-use = 使用

# 网络扫描发现的打印机数量，显示在结果列表上方。
discover-found-count = { $count ->
    [0] 未找到打印机
   *[other] 找到 { $count } 台打印机
}

# 当扫描结果为空时代替结果列表显示；具体文本取决于原因（见 `ipc.ts` 中的
# `DiscoveryVerdict`）。实际上 `Found` 无法到达此消息（它意味着至少有一个结果），
# 因此与任何无法识别的值一同回退到默认文本。
discover-verdict-empty = { $verdict ->
    [NoUsableInterface] 未找到可用的网络接口。请检查网络连接后重试。
    [PermissionDenied] PandaSpy 需要权限才能发现本地网络上的设备。请检查系统的隐私设置后重试。
    [NoResponse] 没有打印机响应。请确认打印机已开机并连接到同一网络。
   *[unknown] 未找到任何打印机。
}

add-printer-manual-serial = 序列号
add-printer-manual-address = IP 地址
add-printer-manual-access-code = 访问码
add-printer-manual-nickname = 昵称（可选）
# 在 `getSettings()` 解析完成前的短暂窗口内，`hasKeyring` 为 "no"——此时可以进入
# 添加界面，但尚不知道密钥库的平台名称——于是回退到一句通用说法，而非空的
# “保存在 。” 。
add-printer-manual-access-code-hint = { $hasKeyring ->
    [yes] 可在打印机屏幕上的 设置 → WLAN 中找到它。PandaSpy 会将其保存在 { $keyring } 中，绝不会存入打印机列表文件。
   *[no] 可在打印机屏幕上的 设置 → WLAN 中找到它。PandaSpy 会安全地保存它，绝不会存入打印机列表文件。
}
add-printer-manual-submit = 添加打印机
add-printer-manual-error-required = 请输入序列号和访问码。
add-printer-manual-error = 无法添加此打印机：{ $message }

add-printer-studio-import = 从 Bambu Studio 导入
add-printer-studio-importing = 正在查找 Bambu Studio 中的打印机…
add-printer-studio-empty = 尚未在 Bambu Studio 中找到打印机。
add-printer-studio-use = 使用

## Certificate trust prompt

trust-title = 此打印机的证书已更改
trust-body = PandaSpy 为此打印机固定的证书，与它刚刚提供的证书不再匹配。从这里看，重新刷机与网络中的攻击者完全相同——在做出决定前，请将下方的指纹与打印机屏幕上显示的指纹进行比对。
trust-pinned-label = 先前已固定
trust-presented-label = 当前提供
trust-accept = 匹配——信任它
trust-reject = 保持阻止
trust-error = 无法记录你的决定：{ $message }

## Settings

settings-title = 设置
settings-language = 语言
settings-language-system = 跟随系统
settings-launch-at-login = 登录时启动

settings-secrets = { $backend ->
    [os-keyring] 访问码保存在你系统的 { $keyring } 中。
    [encrypted-file] 没有可用的系统密钥库，因此访问码保存在受你的登录保护的加密文件中。
   *[unknown] 访问码保存在 { $keyring } 中。
}

settings-save-error = 无法保存设置：{ $message }

## Update banner

update-available = { -brand-name } { $version } 已可用。
update-install = 安装并重启
update-installing = 安装中…
update-install-error = 更新失败：{ $message }
update-dismiss = 忽略
