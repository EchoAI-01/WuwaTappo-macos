# WuwaTappo-macos

![platform](https://img.shields.io/badge/platform-macOS%2010.15%2B-blue) ![tauri](https://img.shields.io/badge/Tauri-2-FFC131) ![rust](https://img.shields.io/badge/Rust-Stable-orange) ![release](https://img.shields.io/github/v/release/EchoAI-01/WuwaTappo-macos?include_prereleases&label=release)

[鸣潮](https://mc.kurogames.com/) 桌宠 **WuwaTappo** 的 macOS 社区移植版 —— **非官方，仅为 macOS 重建原生外壳**，完整复用官方 Windows 版前端 dist 与云端协议，让 Mac 用户也能使用桌宠并与 Windows exe 端玩家正常联机互动。

> ⚠️ 本项目处于 Beta 阶段，核心交互依赖 macOS 系统权限（见 [系统权限说明](#系统权限说明运行必读)）。首次使用请先读 [快速上手](#快速上手)。

---

## 目录

- [项目简介](#项目简介)
- [功能特性](#功能特性)
- [快速上手](#快速上手)
- [系统权限说明（运行必读）](#系统权限说明运行必读)
- [架构与运行机制](#架构与运行机制)
- [技术栈](#技术栈)
- [项目结构](#项目结构)
- [配置说明](#配置说明)
- [构建与打包](#构建与打包)
- [前端补丁清单](#前端补丁清单)
- [开发者必读](#开发者必读)
- [诊断工具](#诊断工具)
- [已知问题与限制](#已知问题与限制)
- [版本历史](#版本历史)
- [贡献指南](#贡献指南)
- [License](#license)
- [致谢](#致谢)
- [AI 声明](#ai-声明)
- [免责声明](#免责声明)

---

## 项目简介

官方 WuwaTappo 仅发布了 Windows 安装包（`WuwaTappo-cn_1.0.1_x64-setup.exe`，后续更新至 1.0.3）。本项目通过逆向该安装包：

1. 解壳取出前端 dist（Vue 3 + Vite 编译产物）与未 strip 的 Rust 二进制
2. 分析符号还原官方自定义 Tauri invoke 命令签名与窗口模型
3. 实测官方云端协议链路，确认 macOS 可直连
4. **为 macOS 重建 Tauri 原生壳**（本仓库 `src-tauri/`），按官方签名 1:1 实现命令（当前注册 53 个 invoke 命令：52 个按官方签名重建 + 1 个自研输入桥握手）
5. 对官方前端 dist 做**最小限度补丁**（见 [前端补丁清单](#前端补丁清单)，共 4 类），其余字节不动

**为什么选择「复用前端 + 重建壳」而不是用原生技术重写 UI？**

- 官方前端承载了几乎全部业务逻辑：皮肤系统（260 皮肤 × 分层 PNG）、开箱 / 合成 / 房间玩法、KuroSDK 登录、多语言。重写不可维护，且无法跟随官方更新。
- 官方后端本身是 Tauri 2（Rust），命令签名可从二进制直接读取，重建面远小于预估。
- 云端协议（REST + JSON + socket.io）与平台无关，macOS 直连即可与 exe 端互通。

逆向分析材料（exe 原件、完整分析报告）**不在本仓库内**，仅保留实现结论；详细架构与踩坑记录见 [CODE_WIKI.md](CODE_WIKI.md)。

---

## 功能特性

> 当前壳版本 **1.0.3-beta**（跟随官方 1.0.3 前端资产；beta 后缀表示社区移植测试版）。可执行安装包见 [GitHub Releases](https://github.com/EchoAI-01/WuwaTappo-macos/releases)。

| 功能 | 状态 | 说明 |
|---|---|---|
| 桌宠显示 / 缩放 / 透明度 / 置顶 | ✅ | 皮肤资源直连官方 CDN；main 窗口恒置顶（NSWindow floating 层） |
| 换装 / 配饰 / 表情 | ✅ | 官方 appearance 资产（260 皮肤 / 135 配饰 / 504 表情） |
| 原生托盘菜单 | ✅ | 托盘入口、多语言文案同步；桌宠窗口不占 Dock（`LSUIElement`） |
| 背包 / 图鉴 / 兑换 / 设置面板 | ✅ | 1240×800 无边框透明窗口（panel） |
| 桌宠拖拽 / 点击 / 右键菜单 | ✅ 需双权限 | 依赖「设备控制和数据访问（旧称辅助功能）+ 输入监控」进入 Default tap 吞事件模式；缺权限降级为 ListenOnly（点击穿透桌面） |
| 打字 / 点击计数 | ✅ 需输入监控 | 全局键盘计数；持久化于 `native-store/store.json`，重启不丢 |
| 登录（KuroSDK 国服） | ✅ 已打通 | SDK 登录 → queryPlayer → auth/login → infoToken；cookie shim 持久化，重启保持登录态 |
| 联机房间（与 exe 端同房间） | ✅ 已验证 | 本地 WS 中继代理重写 Origin 绕过服务端白名单 403（见 [房间 socket 代理](#房间-socket-代理macos-特有)）；创建 / 加入房间实测可用 |
| 开箱 / 合成 / 收藏 / 兑换码 | ✅ 已验证 | 官方玩法 API；开箱、兑换码经维护者实测可用（2026-11-02） |
| 游戏模式（1.0.3） | ✅ | 检测其他应用全屏（CGWindowList）→ 自动进入/退出游戏模式（禁用桌宠交互）；亦可快捷键手动切换 |
| 自定义快捷键（1.0.3） | ✅ | 默认 F7=游戏模式、F8=切换屏幕、F9=显示/隐藏桌宠；设置页可改键（全局触发经 CGEventTap 匹配，录入期间自动暂停） |
| 登录窗口拖拽（1.0.3） | ✅ | 前端 `startDragging`（capabilities 已含权限） |
| 开机自启动 | ✅ | LaunchAgent（`tauri-plugin-autostart`） |
| 自动更新 | ❌ | 官方 updater 无 darwin 通道，恒显示"无更新"；跟随官方版本需手动换 dist 重编译 |

---

## 快速上手

### 方式一：安装 Release 版（推荐）

1. 到 [Releases](https://github.com/EchoAI-01/WuwaTappo-macos/releases) 下载最新 `WuwaTappo_*_aarch64.dmg` / `x64.dmg`（或通用包），挂载后拖入 `Applications`。
2. 首次打开：adhoc 签名未公证，Gatekeeper 会拦截 → **右键 App → 打开** → 确认（或到 系统设置 → 隐私与安全性 底部点"仍要打开"）。
3. 按 [系统权限说明](#系统权限说明运行必读) 授予两项权限（首次启动会自动弹出授权请求并跳转设置页）。
4. 菜单栏出现托盘图标、桌面右下角出现桌宠即启动成功。

### 使用示例

| 想做的事 | 操作 |
|---|---|
| 登录 | 点击桌宠的**宝箱**或**联机**按钮（或托盘菜单任意面板入口）→ 弹出登录窗 → 账号/扫码登录。登录态跨重启保持 |
| 联机房间 | 登录后 托盘 → 联机房间（或面板内联机页）→ 创建 / 加入房间，与 Windows exe 端同房间互通 |
| 移动桌宠 | 按住桌宠本体拖拽（需双权限；缺失时点击会穿透到桌面） |
| 全局快捷键 | `F7` 游戏模式 / `F8` 切换屏幕 / `F9` 显示隐藏桌宠；面板 → 设置 → 快捷键 可改键 |
| 托盘菜单 | 打开背包 / 图鉴 / 兑换 / 联机房间；桌宠大小（50%–200%）、不透明度（20%–100%）、点击穿透、重启、退出 |
| 桌宠右键 | 右键桌宠底座弹出与托盘同构的原生菜单 |
| 游戏模式 | 其他应用进入全屏时自动启用（禁用桌宠交互），退出全屏自动恢复 |

### 方式二：源码构建运行

```bash
# 1. 克隆仓库
git clone https://github.com/EchoAI-01/WuwaTappo-macos.git
cd WuwaTappo-macos

# 2. 开发模式（编译并启动 .app）
cargo tauri dev

# 3. 或用 LaunchServices 启动已构建产物（重要，见下方注意事项）
open src-tauri/target/release/bundle/macos/WuwaTappo.app
```

**开发注意事项**：

1. **前端嵌入是编译期的**：Tauri 2 的 `frontendDist` 经 `build.rs` + `generate_context!` 在编译期嵌入二进制，cargo 增量编译**不会**因 `frontend/` 变化自动重嵌入。修改前端后必须：
   ```bash
   touch src-tauri/src/lib.rs && cargo tauri dev
   ```
2. **WKWebView 磁盘缓存**：`~/Library/Caches/com.kuro.wuwatappo.cn` 会缓存 `tauri://localhost` 的 JS。调试 UI 异常时先 `rm -rf ~/Library/Caches/com.kuro.wuwatappo.cn`（**不要**删 `~/Library/WebKit/com.kuro.wuwatappo.cn/WebsiteData`，那是 localStorage / 登录态）。
3. **始终用 `open *.app`（LaunchServices）启动测试**。直接执行二进制 / `nohup` 会触发 WKWebView idle 挂起伪影，产生假 bug。
4. **单实例**：房间 socket 代理独占 `127.0.0.1:17321`，双开时后启动实例的房间按钮会置灰。

---

## 系统权限说明（运行必读）

本项目的拖拽 / 点击 / 全局计数依赖 CGEventTap 全局键鼠监听，需要两层 TCC 授权（Info.plist 已内置用途声明文案）：

| 权限 | 用途 | 缺失表现 |
|---|---|---|
| **设备控制和数据访问**（macOS 27+；旧系统称**辅助功能 / Accessibility**） | CGEventTap 基础能力 + 吞事件 | tap 无法建立 → 全局事件收不到；或仅 ListenOnly → 点击穿透桌面 |
| **输入监控（Input Monitoring）** | Default tap 模式（可吞事件）与键盘观测 | 点击穿透桌面、键盘不计数、拖拽间歇失效 |

> **macOS 27 设置项改名**（26B123 实测）：「隐私与安全性」下的「辅助功能」条目已移除，合并进新条目 **「设备控制和数据访问」**（面板说明含"监控键盘、录屏、控制 Mac 上的任何 App"）；旧深链 `x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility` 会自动跳转到新面板。「输入监控」仍为独立条目。macOS 26 及更早系统仍叫「辅助功能」。侧边栏的「无障碍」是辅助*功能*设置页，与授权无关。

- 首次运行系统会弹授权请求；未弹出时手动前往 **系统设置 → 隐私与安全性 → 设备控制和数据访问 / 输入监控** 勾选 WuwaTappo（列表无条目时可点「+」添加）。
- 授权后**无需重启 App**：worker 线程每 3s 轮询权限，自动建立 / 升级为 Default tap。
- 授权状态自检：`cat /tmp/wuwatappo-input.log` —— `default/swallow` = 完整模式；`listen-only` = 缺权限（文件内附具体缺哪一项与 binary 路径）。
- **adhoc 签名的特殊性**：`signingIdentity: "-"` 无开发者证书，重编译 / 换路径后 cdhash 变化，TCC 授权**可能**失效需重新勾选（实测表现因系统版本而异）。使用固定证书签名可使授权跨编译保持（见 [构建与打包](#构建与打包)）。

---

## 架构与运行机制

### 总览

```
┌────────────────────────────────────────────────────────────┐
│  官方云服务（Kuro）                                          │
│  配置发现 / 资源CDN / KuroSDK登录 / 玩法API / 房间 socket.io   │
└──────────────────────▲─────────────────────────────────────┘
                       │ REST + JSON / wss（与 exe 端同一协议）
┌──────────────────────┴─────────────────────────────────────┐
│  前端 dist（官方 Vue3+Vite 产物，编译期嵌入二进制）            │
│  index.html + hash 路由：#/  #/panel  #/login  #/about…      │
└──────────────────────▲─────────────────────────────────────┘
                       │ Tauri IPC（53 个 invoke 命令）
┌──────────────────────┴─────────────────────────────────────┐
│  macOS 原生壳（本仓库 src-tauri/，Rust）                     │
│  ├─ 窗口模型：main 全屏 shell + panel/login/toast 等辅助窗口  │
│  ├─ CGEventTap 全局键鼠监听 + 全局快捷键匹配（input.rs）      │
│  ├─ 房间 socket.io 本地中继代理（ws_proxy.rs）                │
│  ├─ 前台全屏检测（fullscreen.rs，游戏模式数据源）              │
│  ├─ 托盘菜单 / 计数与快捷键持久化 / 多显示器                   │
│  └─ keep-alive 线程（防 WKWebView idle 挂起）                 │
└────────────────────────────────────────────────────────────┘
```

### 窗口模型

- **main 窗口**：全屏透明 shell，`ignoresMouseEvents` **恒穿透**、多工作区可见、不占 Dock。`tauri.conf.json` 中 320×420 仅是初始值，启动后由前端 fallback 链路调整为全屏。桌宠本体绘制在右下角 CSS 区域。
- **辅助窗口**（panel / login / toast / update / about / oss-notice）：独立无边框透明窗口，共用同一个 `index.html`，靠 hash 路由区分页面，不穿透、正常接收点击。全部由 Rust 侧 `create_aux_window_min` 统一创建。

### 交互管线（本移植核心）

macOS 没有 Win32 `WM_NCHITTEST` per-region 窗口级命中测试，因此按权限分两条路径：

**路径 A（首选，需双权限）：Default tap 吞事件**

```
main 窗口恒穿透
CGEventTap（Default，可修改事件）
  │ MouseDown → hit_test()
  │   命中桌宠区域 → emit mouse-button(press) → return null（吞掉，桌面收不到）
  │   未命中       → emit mouse-button(press) → 原样透传
  ▼
前端 mouse-move + elementFromPoint 合成点击 / beginDockPointerAt 合成拖拽
  / 右键 → invoke popup_main_dock_context_menu 弹原生菜单
  ▼
前端把桌宠可交互区域经 set_main_hit_region 上报 Rust
（inputRegions + regions，字段 left/top/width/height）
```

**路径 B（降级，仅单权限）：窗口动态穿透**

```
Pointer 进入桌宠区域 → main.set_ignore_cursor_events(false)（窗口开始接收鼠标）
Pointer 离开桌宠区域 → main.set_ignore_cursor_events(true)（恢复穿透）
```

两条路径共用同一套前端事件协议：`key-press` / `key-release`（`KeyboardEvent.key` 风格字符串）、`mouse-move`（全局物理像素 `{x, y}`）、`mouse-button`（`{button, action}`）。

### CGEventTap 线程模型（零阻塞回调）

```
global-input worker 线程
  ├── 权限探测/请求 → 建立 CGEventTap（HIDEventTap + HeadInsertEventTap）
  └── CFRunLoop 分片运行（50ms/片，响应 tap 超时重启与权限升级）
        └── event_tap_callback（必须微秒级返回）
              └── 纯内存判断 + try_send(TapMsg) → 绝不阻塞

input-emitter 线程（独立）
  └── 从 sync_channel(1024) 取 TapMsg
        ├── 按下时先匹配全局快捷键表 → 命中 emit "app-shortcut" 并吞掉该键
        └── 其余 emit_to 前端（key-press / mouse-move / mouse-button）
```

**为什么必须通道化**：CGEventTap 回调超过系统阈值（约 0.25s）时事件会被系统直接丢弃；KeyUp 一旦丢失，系统认为按键一直按着 → 键盘重复（退出时疯狂刷屏）。回调内禁止任何 `emit_to` / 写盘 / `Mutex::lock`。tap 被系统禁用重启时，按按下位图补发 key-release，防前端计数集合永久残留。

### WKWebView keep-alive（macOS 特有）

WKWebView 会对"无活动"页面执行 **idle 挂起**（JS 冻结、timer 停转），表现为点开登录窗后 main 页面冻结、按键统计停止。Rust 侧后台线程每 5 秒对所有 WebView 注入 `void 0` eval 维持活跃。

### NSWindow level 物理分层（macOS 特有）

Tauri `set_always_on_top` 只能在 normal(0)/floating(3) 两档切换。本项目：

- main 窗口 **恒 floating(3)** 永不降级（`sync_main_layer`，幂等）
- 辅助窗口创建 / show 后用 `objc2::msg_send![nsWindow, setLevel: 4]`（`ensure_aux_level`）抬到 floating+1，**恒压 main 之上**

（任何对 aux 再调 `set_always_on_top(true)` 会把 level 重置回 3，必须补调 `ensure_aux_level`）

### 房间 socket 代理（macOS 特有）

联机房间走 socket.io 长连接（`wss://api-mc-wuwatappo.kurogames-global.com/socket.io`）。服务端握手校验 **Origin 白名单**（curl 实锤）：官方 Windows 的 `http://tauri.localhost` → 101 放行；macOS WKWebView 的 `tauri://localhost` → **403 拒绝**；无 Origin → 放行。WebSocket 握手 Origin 由 WKWebView 自动携带、前端 JS 无法修改 → socket 连不上 → 创建/加入房间按钮置灰（前端前置检查 `socket_disconnected`）。

修复（`src-tauri/src/ws_proxy.rs`）：Rust 本地监听 `127.0.0.1:17321`，前端 socket 默认地址补丁指向代理；代理 accept 后以 `Origin: http://tauri.localhost` 外连官方 wss，双向透传帧（path/query 含 infoToken 凭据原样转发，日志仅记录 path 部分脱敏）。页面 origin 为非 secure context，连 `ws://` 回环无混合内容限制；代理只绑回环、目标 host 硬编码，无额外攻击面。

### 全局快捷键与游戏模式（1.0.3）

- **快捷键**：配置存 `native-store/store.json` 的 `app_shortcuts` 键（默认 F7/F8/F9）。binding 结构对齐前端：`modifiers` 位编码 Alt:1｜Ctrl:2｜Shift:4｜Meta:8，`key` 为 DOM keyCode（F 键 112–123、字母大写 ASCII、数字 48–57）。CG 键码/标志经 `dom_key_from_cg` / `dom_mods_from_cg` 换算后在 emitter 线程匹配（`match_shortcut`），命中 emit `app-shortcut` 给 main；配置变更经 `app-shortcuts-changed` 广播。设置页录入按键期间 `set_shortcut_capture` 暂停全局触发。非 F 键必须带 Ctrl/Alt/Meta（与前端约束一致）；重复绑定返回错误串 `"duplicate"`（前端精确匹配）。
- **游戏模式**：`get_main_desktop_safe_area` 返回 `foregroundApplicationFullscreen`（`fullscreen.rs` 用 CGWindowList 检测：存在 layer==0、非本进程、bounds 覆盖主显示器的在屏窗口），前端 `autoGameModeOnFullscreen` 据此自动进入/退出游戏模式。窗口列表查询不需要屏幕录制权限。

---

## 技术栈

| 层 | 技术 | 版本 | 说明 |
|---|---|---|---|
| 原生壳 | [Tauri](https://tauri.app/) | ^2.10.3 | 启用 tray-icon、macos-private-api、protocol-asset |
| 原生壳 | Rust | Edition 2021 | 53 个 invoke 命令（`#[tauri::command]` + `generate_handler!`） |
| 系统监听 | CoreGraphics / CoreFoundation | — | `objc2-core-graphics` 0.3.2 + `objc2-core-foundation` 0.3.2（CGEventTap / CGWindowList） |
| 窗口层级 | objc2 | 0.6 | `NSWindow setLevel` 手动调用 |
| 房间代理 | tokio + tokio-tungstenite + futures-util | 1.x / 0.26 / 0.3 | rustls-tls-native-roots；本地 ws 中继 |
| 插件 | tauri-plugin-opener / process / dialog / deep-link / autostart / updater | 2.x | autostart 使用 `MacosLauncher::LaunchAgent` |
| 前端 | 官方 Vue 3 + Vite dist | 编译期嵌入 | 仅包含编译后的 JS / CSS / 图片 / 字体，无源码 |
| 构建 | cargo-tauri | 随 Tauri 2 | `cargo tauri build` / `cargo tauri dev` |
| 序列化 | serde / serde_json | 1.x | — |
| Bundle | macOS app + dmg | — | adhoc 签名（`signingIdentity: "-"`），无公证 |

---

## 项目结构

```
WuwaTappo-macos/
├── CODE_WIKI.md                      # 详细架构与踩坑记录（含行号快照、事件协议、BUG 诊断）
├── README.md                         # 本文件
├── .gitignore
│
├── frontend/                         # 官方前端编译产物（dist，原样复用 + 最小补丁）
│   ├── index.html                    # SPA 入口（含 document.cookie shim 注入）
│   └── assets/                       # 125 个文件：JS chunk / CSS / 字体 / 图片（已编译）
│
└── src-tauri/                        # Rust / Tauri 原生壳
    ├── Cargo.toml                    # Rust 依赖清单（version = 壳版本）
    ├── Cargo.lock
    ├── tauri.conf.json               # Tauri 配置（窗口、bundle、macOSPrivateApi、签名）
    ├── Info.plist                    # macOS Bundle 配置（LSUIElement、权限声明、ATS）
    ├── build.rs                      # Tauri 标准构建脚本（编译期嵌入 frontendDist）
    ├── capabilities/
    │   └── main.json                 # Tauri 2 ACL 权限声明（166 条，覆盖 10 个窗口 label）
    ├── gen/schemas/                  # Tauri 自动生成的 JSON Schema（gitignore）
    ├── icons/                        # 应用图标（多尺寸）
    └── src/
        ├── main.rs                   # 可执行入口（main → lib::run）
        ├── lib.rs                    # 应用主逻辑：窗口模型 / 53 命令 / 托盘 / 快捷键系统 / 登录补发 / keep-alive
        ├── input.rs                  # CGEventTap 全局键鼠监听 + 命中测试 + 快捷键触发匹配
        ├── fullscreen.rs             # 前台应用全屏检测（游戏模式数据源，CGWindowList FFI）
        └── ws_proxy.rs               # 房间 socket.io 本地中继代理（Origin 白名单绕过）
```

> **注意**：仓库不包含前端源代码（Vue/TS），仅包含编译后的 dist。若需重构建前端，需从官方安装包逆向复现提取流程（不在本仓库覆盖范围内）。

---

## 配置说明

### tauri.conf.json（构建期）

| 键 | 当前值 | 说明 |
|---|---|---|
| `productName` / `version` | `WuwaTappo` / `1.0.3-beta` | 壳版本跟随官方前端版本 + beta 后缀 |
| `identifier` | `com.kuro.wuwatappo.cn` | 与官方一致；WebKit 数据目录 / TCC 展示名按此解析 |
| `build.frontendDist` | `../frontend` | 编译期嵌入官方 dist |
| `app.macOSPrivateApi` | `true` | 透明窗口等私有 API 必需 |
| `app.windows[0]`（main） | 320×420、transparent、无装饰、alwaysOnTop、skipTaskbar | 仅初始值，启动后由前端链路调整为全屏 shell |
| `app.security.csp` | `null` | 与官方一致（前端需加载官方 CDN 脚本） |
| `bundle.targets` | `["app", "dmg"]` | — |
| `bundle.macOS.signingIdentity` | `"-"` | adhoc 签名；换固定证书见 [构建与打包](#构建与打包) |
| `bundle.macOS.minimumSystemVersion` | `10.15` | — |
| `plugins.updater.endpoints` | `[]` | 官方无 darwin 更新通道，保持空 |

### Info.plist（运行时系统行为）

| 键 | 作用 |
|---|---|
| `LSUIElement = true` | 不占 Dock，仅托盘入口（对应 Windows 版 skipTaskbar） |
| `NSAccessibilityUsageDescription` | 辅助功能 / 设备控制和数据访问 授权弹窗文案 |
| `NSInputMonitoringUsageDescription` | 输入监控授权弹窗文案 |
| `NSAppTransportSecurity.NSAllowsArbitraryLoads = true` | 官方 CDN / API 混合资源加载 |

### capabilities/main.json（Tauri 2 ACL）

镜像官方 exe 的权限集（自二进制提取）：166 条权限、窗口 label 覆盖 `main / panel / login / setting / toast / update / about / oss-notice / installer-shell / *`。新增窗口 label 时需同步此文件，否则前端 invoke 会被 ACL 拒绝。

### 运行时配置（用户数据目录）

`$APPLOCALDATA` = `~/Library/Application Support/com.kuro.wuwatappo.cn/`

| 位置 | 键 / 文件 | 说明 |
|---|---|---|
| `native-store/store.json` | `key_press` | 打字计数 `{count, environment}`（environment 不匹配自动归零） |
| | `cumulative_click` | 累计点击 `{count, cuid, environment}` |
| | `app_shortcuts` | 自定义快捷键配置（缺省回退官方默认 F7/F8/F9） |
| WebKit WebsiteData | localStorage | 登录态（cookie shim 后端 `__kuro_cookie_shim__`，含 `kuro_infoToken`）；**清缓存时勿删** |
| `logs/app-YYYYMMDD.log` | — | 前端经 `write_app_log` 写入的运行日志（含 JS error / 心跳） |

### 编译期常量（改动需前后端对齐）

| 常量 | 位置 | 值 |
|---|---|---|
| `PROXY_PORT` | `ws_proxy.rs` | `17321`（前端 socket 默认地址补丁硬编码同值，两处必须一致） |
| `TARGET_HOST` / `SPOOF_ORIGIN` | `ws_proxy.rs` | 官方房间 host / `http://tauri.localhost` |
| `AUX_WINDOW_LEVEL` | `lib.rs` | `4`（floating+1） |
| `PANEL_SIZE` | `lib.rs` | 1240×800（panel 前端 CSS 设计高 756px，窗口小于此值内容被裁切） |
| `PET_SCALE_STEPS` / `OPACITY_STEPS` | `lib.rs` | 50–200%（7 档）/ 20–100%（5 档） |
| 默认快捷键 | `lib.rs default_shortcuts()` | F7=gameMode、F8=switchScreen、F9=togglePet |

---

## 构建与打包

```bash
# 只出 .app（快，约 2-3 分钟）
cargo tauri build --bundles app

# 出 .app + .dmg（完整分发）
cargo tauri build
```

产物位置：`src-tauri/target/release/bundle/`（`macos/WuwaTappo.app`、`dmg/*.dmg`）。

### 发布匿名化

Release profile 已设 `strip = true / lto / codegen-units=1`。分发构建额外做路径重映射，避免二进制泄露打包机用户名与目录结构（panic 信息 / 符号表中的编译机绝对路径）：

```bash
RUSTFLAGS="--remap-path-prefix=$(pwd)=/build/wuwatappo" cargo tauri build
# 验证：strings 产物不应再出现打包机路径
strings src-tauri/target/release/wuwatappo-macos | grep -c "/Users/"   # 期望 0
```

（cargo 1.99 下 `-C remap-path-prefix` 与 profile `trim-paths` 均不稳定，须用顶层 `--remap-path-prefix` flag。）发布版同时剥离强诊断注入、日志对 token 脱敏（ws 代理仅记录 path 部分）。

### 签名说明

`tauri.conf.json` 中 `"signingIdentity": "-"` 表示 adhoc 签名。若本机已配置开发者证书，可固定签名（避免每次编译 cdhash 变化导致 TCC 授权失效）：

```bash
APPLE_SIGNING_IDENTITY="WuwaTappo Dev Signing" cargo tauri build
```

adhoc 签名产物未公证，他人首次打开需 **右键 → 打开**（绕过 Gatekeeper）。

---

## 前端补丁清单

官方 dist（当前基于 **1.0.3**）除以下补丁外字节不动：

| 文件 | 补丁 | 原因 |
|---|---|---|
| `index.html` | `<head>` 注入 document.cookie shim（localStorage 后端，探测原生 cookie 可用则不启用）；主 bundle/CSS 引用随版本更新 | WKWebView 在 `tauri://localhost` 自定义 scheme 下禁用 `document.cookie`（WebKit 非 http(s) origin 限制）；官方前端登录态存于 `kuro_infoToken` cookie，不打补丁会导致"登录成功但应用仍显示未登录" |
| 全部 `assets/*.js` | 显示名 `"Wuwa Tappo"` → `"WuwaTappo"`（1.0.3 共 102 处） | 应用名统一（与产品名/托盘一致） |
| `assets/index-CuzHFfJW.js`（主 bundle） | `isdkSrc` / `isdkCss` 由 `//pro-cdn-sdk.kurogame.com/...` 改为 `https://...` | protocol-relative URL 在 `tauri://localhost` 下解析失败 → KuroSDK 加载不了 → 登录窗口空白 |
| `assets/index-pp8VWaUF.js`（房间/panel） | ① `key-press` 监听注册后追加 `__TAURI_INTERNALS__.invoke("notify_input_bridge_ready")`；② socket 默认地址函数 `ey()`（1.0.1 名 `Ig()`）改为 `http://127.0.0.1:17321` | ① 输入桥就绪握手观测点（仅日志，不门控）；② 房间 socket.io 服务端 Origin 白名单拒绝 `tauri://localhost`（403），经本地代理重写（见 [房间 socket 代理](#房间-socket-代理macos-特有)） |

---

## 开发者必读

### 关键约束（改相关代码前必读）

**1. 输入桥握手：只能观测，不能门控**
曾按"tap 等前端注册完监听再转发事件"实现门控（防 IPC 轰炸竞态），结果与前端初始化链形成**循环等待**——前端初始化链的 await 依赖首批 mouse-move 事件做布局，门控等握手，握手永远不来 → 初始化链永久中断。已回退：事件始终直转，`notify_input_bridge_ready` 仅作日志观测。

**2. hit_region 字段名是 left/top 不是 x/y**
前端上报矩形用 `left/top/width/height`。Rust 解析只认 `x/y` 时不报错、静默返回 0 区域 → 桌宠点击 / 拖拽全被路由层过滤、事件穿透到桌面。现已双字段兼容（`parse_regions`）。

**3. 计数命令返回格式不一致（官方如此）**
`load_key_press_count` 返回裸 `i64`（前端 `Number(payload)` 解析），`load_cumulative_click_count` 返回对象。重建命令时勿"统一"格式，前端按官方格式解析。

**4. main 窗口恒穿透 + hit_region 双来源**
`input_regions`（可交互：按钮 / 宝箱 / panel 控件）和 `visual_regions`（可视：桌宠身体 / dock）都能命中，**任一命中即视为桌宠区域**。`set_main_hit_region` 内禁止再调 `set_ignore_cursor_events`（恒穿透由启动时 `set_main_force_cursor_ignore(true)` 设置；重复调用失败会让整个命令返回 Err，前端随即跳过区域同步）。

**5. NSWindow level 不回退**
任何对 aux 窗口再调 `set_always_on_top(true)` 会把 level 重置回 3，必须随后补调 `ensure_aux_level`（已在 `raise_panel_window_above_pets`、toast、existed 早退分支处理）。

**6. tap 回调零阻塞**
回调内禁止 `emit_to` / 写盘 / 加锁 / 查窗口 API（窗口查询只在主线程经 `refresh_main_geom` 缓存）。违者事件被系统丢弃 → 键盘重复刷屏或计数永久失效。

### 详细架构与踩坑记录

完整详细文档（行号快照、结构体 / 函数索引、事件协议参考、BUG 现场诊断、官方 1.0.1→1.0.3 差异分析）见 [CODE_WIKI.md](CODE_WIKI.md)。

---

## 诊断工具

调试 / 定位问题时，以下日志文件可直接查看：

| 文件 | 内容 | 用法 |
|---|---|---|
| `/tmp/wuwatappo-input.log` | 输入模块权限与 tap 模式 | `default/swallow` = 完整模式；`listen-only` = 缺权限（附缺哪项与 binary 路径）；`NOT granted` = 未授权 |
| `/tmp/wuwatappo-diag.log` | 窗口 / 状态诊断 | 命令入口、hit-region payload 与解析结果双写；启动前 2 分钟含窗口几何快照；ws 代理 relay 状态 |
| `~/Library/Logs/com.kuro.wuwatappo.cn/`（实为 `$APPLOCALDATA/logs/`） | 应用运行日志 | 由 `write_app_log` 命令写入，`app-YYYYMMDD.log` 格式，含前端 JS error 与 10s 心跳 |
| `$APPLOCALDATA/native-store/store.json` | 计数与快捷键持久化 | 查看 / 手动重置打字与点击计数、快捷键配置 |

**`$APPLOCALDATA`** 按 Bundle ID 解析为 `~/Library/Application Support/com.kuro.wuwatappo.cn/`。

---

## 已知问题与限制

### 已知问题

| # | 问题 | 状态 | 临时解决 |
|---|---|---|---|
| 1 | panel 内「登录」按钮路径偶发不弹登录窗（宝箱 / 联机 / 托盘路径正常） | 开放（低优先级） | 用宝箱 / 联机入口触发登录 |
| 2 | 重编译 / 换路径后 TCC 授权可能失效（adhoc cdhash 变化）；macOS 27 上授权条目名为「设备控制和数据访问」而非「辅助功能」，老教程会对不上 | 开放 | 看 `/tmp/wuwatappo-input.log` 确认模式；按 [权限说明](#系统权限说明运行必读) 重新勾选 |
| 3 | 开发目录版与 /Applications 版同名同图标，易启错实例；且代理端口 17321 单实例独占，双开时后启实例房间按钮置灰 | 使用注意 | 启动前确认已退出另一实例 |

### 已修复（历史问题归档）

| 问题 | 修复 | 版本 |
|---|---|---|
| 登录浮层不渲染（透明窗不可见） | 补发官方 `login-modal-open` 事件 + intent 透传 + cookie shim 持久化登录态 + auth 链防腰斩（close 延迟 1200ms） | 1.0.1-beta |
| 未聚焦时键盘不计数 / 拖拽点击间歇失效 | 输入监控授权引导 + Default tap 吞事件 + 事件转发改异步 channel（防回调超时丢 KeyUp）+ tap 重启补发 release | v0.0.1-Beta → 1.0.1-beta |
| 桌宠被其他窗口覆盖 | NSWindow level 物理分层：main 恒 floating(3)、aux 恒 4 | 1.0.1-beta |
| 联机房间按钮置灰 | 本地 WS 中继代理重写 Origin（服务端白名单 403） | 1.0.1-beta |
| 点开登录窗后 main 冻结（统计停止） | WKWebView keep-alive eval | v0.0.1-Beta |

### 已知限制

- **自动更新**：官方 updater 无 darwin 通道，恒"无更新"；跟随官方版本需手动更新前端 dist 并重编译壳。
- **联机房间 / 开箱等玩法**：依赖官方服务端可用性，与官方 exe 端同源同命运。
- **键码映射**：小键盘 / JIS 键盘映射按 BongoCat 表校准，未在全键盘布局实测。
- **dmg 未公证**：adhoc 签名，他人下载首次打开需右键 → 打开。
- **前端版本绑定**：壳版本严格绑定 `frontend/` 中的 dist 版本；官方更新前端后需同步逆向提取新 dist 并重新对齐命令签名。

---

## 版本历史

| 版本 | 日期 | 内容 | Release |
|---|---|---|---|
| 1.0.3-beta | 2026-11-02 | 跟随官方 1.0.3 前端：游戏模式（CGWindowList 全屏检测）、自定义快捷键（F7/F8/F9 可改键，全局触发 + 录入保护）、登录窗拖拽、新美术资源；4 项移植补丁全量平移 | [v1.0.3-beta](https://github.com/EchoAI-01/WuwaTappo-macos/releases) |
| 1.0.1-beta | 2026-11-02 | 首个登录+联机全打通版本：cookie shim 登录态持久化、login-modal-open 补发与 intent 透传、NSWindow level 物理分层（桌宠恒置顶）、房间 socket.io 本地中继代理（Origin 白名单绕过）、auth 链防腰斩；发布版匿名化（路径重映射/诊断剥离） | [v1.0.1-beta](https://github.com/EchoAI-01/WuwaTappo-macos/releases) |
| v0.0.1-Beta | 2026-10-28 | 首个开源版本：壳重建完成；修复 hit_region 字段解析（left/top）、KrSDK https 加载、计数持久化、WKWebView keep-alive、面板 1240×800 适配；修复退出时键盘重复刷屏（事件转发改为异步 channel） | — |

---

## 贡献指南

当前 Beta 阶段，欢迎：

- 反馈 Bug 与功能建议（[Issues](https://github.com/EchoAI-01/WuwaTappo-macos/issues)）；报 bug 时附 `/tmp/wuwatappo-input.log` 与 `/tmp/wuwatappo-diag.log` 可大幅加速定位
- 贡献 macOS 权限 / 签名 / Tauri 相关修复
- 补充逆向分析中的命令签名或窗口模型细节
- 更多玩法场景与键盘布局的实测反馈

流程：fork → 分支 → PR。代码风格遵循 `cargo fmt` + `cargo clippy` 默认规则。**请勿手改二进制产物**（`frontend/assets/*`）；前端补丁应保持在 [补丁清单](#前端补丁清单) 的最小集合内，新增补丁需登记原因与锚点。修改 `src-tauri/src/lib.rs` 之外的前端相关文件后记得 `touch src-tauri/src/lib.rs` 触发重嵌入。

---

## License

本仓库**未包含 LICENSE 文件**，自研壳代码（`src-tauri/` 与补丁脚本）未授予任何开源许可证，默认保留所有权利；二次分发或商业使用前请联系项目维护者确认。

仓库内同时包含不受上述声明覆盖的第三方内容：

- **官方前端 dist**（`frontend/`）：版权归库洛游戏 / Kuro Games 所有，本项目仅原样复用其公开客户端编译产物；
- [BongoCat](https://github.com/ayangweb/BongoCat)（Apache-2.0）：`input.rs` 的 CGEventTap 挂法与键码映射表参考其实现；
- Tauri 及其插件（MIT / Apache-2.0）、tokio / tungstenite 等依赖按各自许可证使用。

---

## 致谢

- **库洛游戏（Kuro Games）**：WuwaTappo 本体与云端服务
- [BongoCat](https://github.com/ayangweb/BongoCat)（Apache-2.0）：macOS CGEventTap 挂法（HIDEventTap + HeadInsertEventTap）与键码 → HID usage 映射表参考
- [Tauri](https://tauri.app/) 社区：跨平台 WebView 壳框架

---

## AI 声明

本项目的移植开发由 **AI 完成**：逆向分析、macOS 原生壳重建（本仓库全部 Rust 代码）、前端最小补丁、文档（README / CODE_WIKI）与打包发布流程，均由 AI（Claude，Anthropic）在维护者的指导与决策下生成；维护者负责功能实测验收（登录、联机、开箱、兑换码、权限与安装流程）、关键方案取舍与发布确认。

AI 生成的代码与文档虽经真机验证，仍可能存在发布时未知的缺陷；遇到问题请查阅 [已知问题与限制](#已知问题与限制) 并提交 Issue（附诊断日志可加速定位）。

---

## 免责声明

本项目为社区移植，与库洛游戏 / Kuro Games 无关。鸣潮（Wuthering Waves）及相关资产版权归其所有者所有。

本项目**不破解、不代理、不重实现任何官方服务**——仅复用官方公开客户端资产并直连官方服务端，对官方 API 不做任何逆向修改；房间 socket 中继仅在本机回环重写握手 Origin 头，不接触、不存储凭据内容。

本项目仅供个人学习研究使用；若官方提出异议，本项目将立即下架处理。
