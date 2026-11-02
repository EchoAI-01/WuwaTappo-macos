# WuwaTappo-macos

![platform](https://img.shields.io/badge/platform-macOS%2010.15%2B-blue) ![tauri](https://img.shields.io/badge/Tauri-2-FFC131) ![rust](https://img.shields.io/badge/Rust-Stable-orange) ![status](https://img.shields.io/badge/status-beta%20测试中-yellow)

[鸣潮](https://mc.kurogames.com/) 桌宠 **WuwaTappo** 的 macOS 社区移植版 —— **非官方，仅为 macOS 重建原生外壳**，完整复用官方 Windows 版前端 dist 与云端协议，让 Mac 用户也能使用桌宠并与 Windows exe 端玩家正常联机互动。

> ⚠️ 本项目处于 Beta 阶段，部分功能依赖 macOS 系统权限，详见下方「运行必读」与「已知问题」。

---

## 目录

- [移植背景](#移植背景)
- [功能特性与状态](#功能特性与状态)
- [架构与运行机制](#架构与运行机制)
- [技术栈](#技术栈)
- [目录结构](#目录结构)
- [环境要求与前置依赖](#环境要求与前置依赖)
- [安装与开发运行](#安装与开发运行)
- [构建与打包](#构建与打包)
- [前端补丁清单](#前端补丁清单)
- [开发者必读](#开发者必读)
- [诊断工具](#诊断工具)
- [已知问题](#已知问题)
- [已知限制](#已知限制)
- [版本历史](#版本历史)
- [贡献指南](#贡献指南)
- [License](#license)
- [致谢](#致谢)
- [免责声明](#免责声明)

---

## 移植背景

官方 WuwaTappo 仅发布了 Windows 安装包（`WuwaTappo-cn_1.0.1_x64-setup.exe`）。本项目通过逆向该安装包：

1. `7zz` 双层解壳取出前端 dist 与未 strip 的 Rust 二进制
2. 分析符号还原 **50 个自定义 Tauri invoke 命令** 与窗口模型
3. 实测官方云端协议链路，确认 macOS 可直连
4. **为 macOS 重建 Tauri 原生壳**（本仓库 `src-tauri/`），按官方签名 1:1 实现命令
5. 对官方前端 dist 做**最小限度补丁**（见 [前端补丁清单](#前端补丁清单)），其余字节不动

**为什么选择「复用前端 + 重建壳」而不是用原生技术重写 UI？**

- 官方前端（Vue 3 + Vite 编译产物）承载了几乎全部业务逻辑：皮肤系统（260 皮肤 × 分层 PNG）、开箱 / 合成 / 房间玩法、KuroSDK 登录、多语言。重写不可维护，且无法跟随官方更新。
- 官方后端本身是 Tauri 2（Rust），命令签名可从二进制直接读取，重建面远小于预估。
- 云端协议（REST + JSON）与平台无关，macOS 直连即可与 exe 端互通。

逆向分析材料（exe 原件、完整分析报告）**不在本仓库内**，仅保留实现结论。

---

## 功能特性与状态

> 版本说明：仓库壳版本标记为 **v0.0.1-Beta**；`Cargo.toml` / `tauri.conf.json` 中的版本号 `1.0.1` 沿袭官方前端资产版本，便于对应官方功能集。

| 功能 | 状态 | 说明 |
|---|---|---|
| 桌宠显示 / 缩放 / 透明度 / 置顶置底 | ✅ | 皮肤资源直连官方 CDN |
| 换装 / 配饰 / 表情 | ✅ | 官方 appearance 资产（260 皮肤 / 135 配饰 / 504 表情） |
| 原生托盘菜单 | ✅ | 托盘入口、多语言同步；桌宠窗口不占 Dock |
| 背包 / 图鉴 / 兑换 / 设置面板 | ✅ | 1240×800 无边框透明窗口 |
| 桌宠拖拽 / 点击 / 右键菜单 | ⚠️ 需双权限 | 依赖「辅助功能 + 输入监控」授权进入 Default tap 模式；无输入监控时降级为 ListenOnly（点击穿透、拖拽间歇失效） |
| 打字 / 点击计数 | ⚠️ 需双权限 | 全局键盘计数依赖「输入监控」授权；持久化于 `native-store/store.json`，重启不丢 |
| 登录（KuroSDK 国服） | ✅ 已打通 | SDK 登录 → queryPlayer → auth/login → infoToken（cookie shim 持久化，重启保持登录态） |
| 联机房间（与 exe 端同房间） | ⚠️ 待实测 | socket.io 服务端 Origin 白名单拒绝 `tauri://localhost`（403），已加本地 WS 中继代理重写 Origin（见「房间 socket 代理」） |
| 开箱 / 合成 / 收藏 / 兑换码 | ⚠️ 待实测 | 官方玩法 API，完整验证依赖登录 |
| 自动更新 | ❌ | 官方 updater 无 darwin 通道，恒显示"无更新" |
| 开机自启动 | ✅ | LaunchAgent（`tauri-plugin-autostart`） |

---

## 架构与运行机制

### 总览

```
┌────────────────────────────────────────────────────────────┐
│  官方云服务（Kuro）                                          │
│  配置发现 / 资源CDN / KuroSDK登录 / 玩法API / 房间             │
└──────────────────────▲─────────────────────────────────────┘
                       │ REST + JSON（与 exe 端同一协议）
┌──────────────────────┴─────────────────────────────────────┐
│  前端 dist（官方 Vue3+Vite 产物，编译期嵌入二进制）            │
│  index.html + hash 路由：#/  #/panel  #/login  #/about…      │
└──────────────────────▲─────────────────────────────────────┘
                       │ Tauri IPC（50 个按官方签名 1:1 重建的命令）
┌──────────────────────┴─────────────────────────────────────┐
│  macOS 原生壳（本仓库 src-tauri/，Rust）                     │
│  ├─ 窗口模型：main 全屏 shell + panel/login/toast 等辅助窗口  │
│  ├─ CGEventTap 全局键鼠监听（input.rs）                      │
│  ├─ 托盘菜单 / 计数持久化 / 多显示器                          │
│  └─ keep-alive 线程（防 WKWebView idle 挂起）                │
└────────────────────────────────────────────────────────────┘
```

### 核心运行机制（macOS 移植特有）

#### 窗口模型

- **main 窗口**：2560×1440 全屏透明 shell，`ignoresMouseEvents` **恒穿透**、多工作区可见、不占 Dock（`LSUIElement=true`）。`tauri.conf.json` 中 320×420 仅是初始值，启动后由前端 fallback 链路调整为全屏。桌宠本体绘制在右下角 CSS 区域。
- **辅助窗口**（panel / login / toast / update / about / oss-notice）：独立无边框透明窗口，共用同一个 `index.html`，靠 hash 路由区分页面，不穿透、正常接收点击。全部由 Rust 侧 `create_aux_window_min` 统一创建。

#### 交互管线（本移植核心）

macOS 没有 Win32 `WM_NCHITTEST` per-region 窗口级命中测试，因此按权限分两条路径：

**路径 A（首选，需「输入监控」权限）：Default tap 吞事件**

```
main 窗口恒穿透
CGEventTap（Default，可修改事件）
  │ MouseDown → hit_test()
  │   命中桌宠区域 → emit mouse-button(press) → return null（吞掉，桌面收不到）
  │   未命中       → emit mouse-button(press) → 原样透传
  ▼
前端 mouse-move + elementFromPoint 合成点击 / beginDockPointerAt 合成拖拽
  / fc() → 右键 → invoke popup_main_dock_context_menu
  ▼
前端把桌宠可交互区域经 set_main_hit_region 上报 Rust
（inputRegions + regions，字段 left/top/width/height）
```

**路径 B（降级，仅「辅助功能」权限）：窗口动态穿透**

```
Pointer 进入桌宠区域 → main.set_ignore_cursor_events(false)（窗口开始接收鼠标）
Pointer 离开桌宠区域 → main.set_ignore_cursor_events(true)（恢复穿透）
```

两条路径共用同一套前端事件协议：`key-press` / `key-release`（字符串）、`mouse-move`（全局物理像素 `{x, y}`）、`mouse-button`（`{button, action}`）。

#### CGEventTap 线程模型（零阻塞回调）

```
global-input worker 线程
  ├── 权限探测 → 建立 CGEventTap（HIDEventTap + HeadInsertEventTap）
  └── CFRunLoop 分片运行（50ms/片）
        └── event_tap_callback（必须微秒级返回）
              └── 纯内存判断 + try_send(TapMsg) → 绝不阻塞

input-emitter 线程（独立）
  └── 从 sync_channel(1024) 取 TapMsg → emit_to 前端
```

**为什么必须通道化**：CGEventTap 回调超过系统阈值（约 0.25s）时，该事件会被系统直接丢弃。一旦 KeyUp 丢失，系统认为按键一直按着 → 触发键盘重复（退出时疯狂刷屏）。回调内禁止任何 `emit_to` / 写盘 / `Mutex::lock`。

#### WKWebView keep-alive（macOS 特有）

WKWebView 会对"无活动"页面执行 **idle 挂起**（JS 冻结、timer 停转），表现为点开登录窗后 main 页面冻结、按键统计停止。Rust 侧后台线程每 5 秒对所有 WebView 注入 `void 0` eval 维持活跃。

#### NSWindow level 物理分层（macOS 特有）

Tauri `set_always_on_top` 只能在 normal(0)/floating(3) 两档切换。本项目：

- main 窗口 **恒 floating(3)** 永不降级
- 辅助窗口创建 / show 后用 `objc2::msg_send![nsWindow, setLevel: 4]` 抬到 floating+1，**恒压 main 之上**

（任何对 aux 再调 `set_always_on_top(true)` 会把 level 重置回 3，必须补调 `ensure_aux_level`）

#### 房间 socket 代理（macOS 特有）

联机房间走 socket.io 长连接（`wss://api-mc-wuwatappo.kurogames-global.com/socket.io`，纯 websocket transport）。服务端握手校验 **Origin 白名单**（curl 实锤）：官方 Windows 的 `http://tauri.localhost` → 101 放行；macOS WKWebView 的 `tauri://localhost` → **403 拒绝**；无 Origin → 放行。WebSocket 握手 Origin 由 WKWebView 自动携带、前端 JS 无法修改 → socket 连不上 → 创建/加入房间按钮置灰（前端前置检查 `reason_code: socket_disconnected`）。

修复（`src-tauri/src/ws_proxy.rs`）：Rust 本地监听 `127.0.0.1:17321`，前端 `Ig()` 补丁指向代理；代理 accept 后以 `Origin: http://tauri.localhost` 外连官方 wss，双向透传帧（path/query 含 infoToken 凭据原样转发）。页面 origin 为非 secure context，连 `ws://` 回环无混合内容限制。

---

## 技术栈

| 层 | 技术 | 版本 | 说明 |
|---|---|---|---|
| 原生壳 | [Tauri](https://tauri.app/) | ^2.10.3 | 启用 tray-icon、macos-private-api、protocol-asset |
| 原生壳 | Rust | Edition 2021 | 自定义命令 50 个（`#[tauri::command]` + `generate_handler!` 注册） |
| 系统监听 | CoreGraphics / CoreFoundation | — | `objc2-core-graphics` 0.3.2 + `objc2-core-foundation` 0.3.2 |
| 窗口层级 | objc2 | 0.6 | `NSWindow setLevel` 手动调用 |
| 插件 | tauri-plugin-opener / process / dialog / deep-link / autostart / updater | 2.x | autostart 使用 `MacosLauncher::LaunchAgent` |
| 前端 | 官方 Vue 3 + Vite dist | 编译期嵌入 | 仅包含编译后的 JS / CSS / 图片 / 字体，无源码 |
| 构建 | cargo-tauri | 随 Tauri 2 | `cargo tauri build` / `cargo tauri dev` |
| 序列化 | serde / serde_json | 1.x | — |
| Bundle | macOS app + dmg | — | 无签名（`signingIdentity: "-"`），无公证 |

---

## 目录结构

```
WuwaTappo-macos/
├── CODE_WIKI.md                      # 详细架构与踩坑记录（11 节，含行号快照）
├── README.md                         # 本文件
├── .gitignore
│
├── frontend/                         # 官方前端编译产物（dist，原样复用 + 最小补丁）
│   ├── index.html                    # SPA 入口（含 cookie shim）
│   └── assets/                       # JS chunk / CSS / 字体 / 皮肤图片（已编译）
│
└── src-tauri/                        # Rust / Tauri 原生壳
    ├── Cargo.toml                    # Rust 依赖清单
    ├── Cargo.lock
    ├── tauri.conf.json               # Tauri 配置（窗口、bundle、macOSPrivateApi）
    ├── Info.plist                    # macOS Bundle 配置（LSUIElement、权限声明、ATS）
    ├── build.rs                      # Tauri 标准构建脚本
    ├── capabilities/
    │   └── main.json                 # Tauri 2 ACL 权限声明（173 条，覆盖 9 个窗口 label）
    ├── gen/schemas/                  # Tauri 自动生成的 JSON Schema（忽略）
    ├── icons/                        # 应用图标（多尺寸）
    └── src/
        ├── main.rs                   # 可执行入口（main → lib::run，6 行）
        ├── lib.rs                    # Tauri 应用主逻辑（1403 行）
        └── input.rs                  # 全局输入监听模块（864 行）
```

> **注意**：仓库不包含前端源代码（Vue/TS），仅包含编译后的 dist。若需重构建前端，需从官方 exe 逆向复现源码（不在本仓库覆盖范围内）。

---

## 环境要求与前置依赖

### 硬件 / 系统

- **macOS 10.15 (Catalina)** 或更高
- Intel 或 Apple Silicon（Tauri 2 原生支持 Universal）

### 开发工具链

| 依赖 | 安装方式 | 说明 |
|---|---|---|
| Rust stable | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` | 1.80+ 即可 |
| Xcode Command Line Tools | `xcode-select --install` | 提供 clang、SDK |
| cargo-tauri | `cargo install tauri-cli --version '^2'` | 提供 `cargo tauri` 子命令 |

### macOS 系统权限（**运行必需**）

本项目依赖 CGEventTap 实现全局键鼠监听，需要两层授权：

| 权限 | 用途 | 缺失表现 |
|---|---|---|
| **辅助功能（Accessibility）** | CGEventTap 基础能力 | tap 无法建立 → 全局事件收不到 |
| **输入监控（Input Monitoring）** | Default tap 模式（可吞事件） | ListenOnly 降级 → 点击穿透桌面、键盘不计数、桌宠拖拽间歇失效 |

Info.plist 已内置权限声明文案。首次运行时系统会弹出授权请求；若未弹出，手动前往 **系统设置 → 隐私与安全性 → 辅助功能 / 输入监控** 勾选本应用。授权后 **无需重启 App**，约 2-3 秒自动生效并升级为完整模式。

> ⚠️ **adhoc 签名的特殊性**：`tauri.conf.json` 中 `signingIdentity: "-"`（无开发者证书），**每次重新编译二进制 cdhash 都会变化**，macOS TCC 会把新二进制当成新应用，之前的权限授权**自动失效**需重新勾选。使用固定证书签名可使授权跨编译保持（见 [构建与打包](#构建与打包)）。

---

## 安装与开发运行

```bash
# 1. 克隆仓库
git clone https://github.com/EchoAI-01/WuwaTappo-macos.git
cd WuwaTappo-macos

# 2. 开发模式（热重载编译，启动 .app）
cargo tauri dev

# 3. 用 LaunchServices 启动（重要：直接执行二进制会触发 WKWebView 挂起伪影）
open src-tauri/target/debug/WuwaTappo.app
```

**首次运行必做**：按上文「系统权限」章节授予辅助功能与输入监控权限。

### 开发注意事项

1. **前端嵌入是编译期的**：Tauri 2 的 `frontendDist` 通过 `build.rs` + `generate_context!` 宏在编译期嵌入二进制。cargo 增量编译**不会**因 `frontend/` 目录变化自动重嵌入。修改前端后必须：
   ```bash
   touch src-tauri/src/lib.rs && cargo tauri dev
   ```

2. **WKWebView 磁盘缓存**：`~/Library/Caches/com.kuro.wuwatappo.cn` 会缓存 `tauri://localhost` 的 JS。同名文件就地修改后旧缓存仍可能生效。调试 UI 异常时先：
   ```bash
   rm -rf ~/Library/Caches/com.kuro.wuwatappo.cn
   ```
   （**不要**删 `~/Library/WebKit/com.kuro.wuwatappo.cn/WebsiteData`，那是 localStorage / 登录态）

3. **测试启动方式**：始终用 `open WuwaTappo.app`（LaunchServices）启动。直接执行 `WuwaTappo` 二进制 / `nohup ./WuwaTappo` 会触发 WKWebView 挂起，产生假 bug。

---

## 构建与打包

```bash
# 只出 .app（快，约 2-3 分钟）
cargo tauri build --bundles app

# 出 .app + .dmg（完整分发）
cargo tauri build
```

产物位置：`src-tauri/target/release/bundle/`

### 签名说明

`tauri.conf.json` 中：

```json
"signingIdentity": "-"
```

表示使用 adhoc 签名。若本机已配置开发者证书，可通过环境变量指定固定签名（避免每次编译 cdhash 变化导致权限失效）：

```bash
APPLE_SIGNING_IDENTITY="WuwaTappo Dev Signing" cargo tauri build
```

adhoc 签名产物未公证，他人首次打开需 **右键 → 打开**（绕过 Gatekeeper）。

---

## 前端补丁清单

官方 dist 除以下补丁外字节不动：

| 文件 | 补丁 | 原因 |
|---|---|---|
| `index.html` | `<head>` 注入 document.cookie shim（localStorage 后端） | WKWebView 在 `tauri://localhost` 自定义 scheme 下禁用 `document.cookie`（WebKit 非 http(s) origin 限制）；官方前端登录态存于 `kuro_infoToken` cookie，不禁用会导致"登录成功但应用仍显示未登录" |
| `assets/index-D8GJxFG7.js` | 应用显示文案 | 应用名适配 |
| | `isdkSrc` / `isdkCss` 由 `//pro-cdn-sdk.kurogame.com/...` 改为 `https://...` | protocol-relative URL 在 `tauri://localhost` 下解析失败 → KuroSDK 加载不了 → 登录窗口空白 |
| `assets/index-DmC0wUEw.js` | `key-press` 监听注册后追加 `notify_input_bridge_ready()` 注入 | 输入桥就绪握手观测点（仅日志，不门控） |
| | `Ig()` 默认 socket 地址由 `https://api-mc-wuwatappo.kurogames-global.com` 改为 `http://127.0.0.1:17321`（本地代理） | 房间 socket.io 服务端 Origin 白名单拒绝 `tauri://localhost`（403），代理外发时重写为官方值（见「房间 socket 代理」） |
| `assets/index-DenkafYz.js` | 文案适配 | 同应用名适配 |

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
`input_regions`（可交互：按钮 / 宝箱 / panel 控件）和 `visual_regions`（可视：桌宠身体 / dock）都能命中，**任一命中即视为桌宠区域**。

**5. NSWindow level 不回退**  
任何对 aux 窗口再调 `set_always_on_top(true)` 会把 level 重置回 3，必须随后补调 `ensure_aux_level`（已在 `raise_panel_window_above_pets`、toast / exited 分支处理）。

### 详细架构与踩坑记录

完整 11 节详细文档（含行号快照、结构体 / 函数索引、事件协议参考、BUG 现场诊断）见 [CODE_WIKI.md](CODE_WIKI.md)。

---

## 诊断工具

调试 / 定位问题时，以下日志文件可直接查看：

| 文件 | 内容 | 用法 |
|---|---|---|
| `/tmp/wuwatappo-input.log` | 输入模块权限状态 | `default` / `swallow` = 完整模式；`listen-only` = 缺输入监控；`NOT granted` = 辅助功能未授权 |
| `/tmp/wuwatappo-diag.log` | 窗口 / 状态诊断 | 所有 50 个命令入口 + hit-region payload / 解析结果双写，前 2 分钟保留详细日志 |
| `~/Library/Logs/com.kuro.wuwatappo.cn/` | 应用运行日志 | 由 `write_app_log` 命令写入，`app-YYYYMMDD.log` 格式 |
| `$APPLOCALDATA/native-store/store.json` | 计数持久化 | 查看 / 手动重置打字与点击计数 |

**`$APPLOCALDATA`** 路径按 Bundle ID 解析为 `~/Library/Application Support/com.kuro.wuwatappo.cn/`。

---

## 已知问题

> 详细根因与修复路线见 [CODE_WIKI.md §11](CODE_WIKI.md)。

| # | 问题 | 状态 | 临时解决 |
|---|---|---|---|
| 1 | **登录 / 联机不可用**：点击后面板关闭并创建 login 窗口，但 KrSDK 跨域脚本报错（`Script error.`）导致登录浮层不渲染、透明窗口不可见 | 根因定位中 | — |
| 2 | **未聚焦时键盘不计数**：全局键盘事件依赖「输入监控」权限，ListenOnly 降级收不到 KeyDown / KeyUp | 权限授权即可恢复 | 授予输入监控权限 |
| 3 | **桌宠拖动 / 点击间歇失效**：同问题 2——ListenOnly 模式无法吞事件，点击穿透桌面；动态穿透切换存在时序竞态 | 权限授权 + Default tap 模式可解大半 | 授予输入监控权限 |

---

## 已知限制

- **自动更新**：官方 updater 无 darwin 通道，恒"无更新"；跟随官方版本需手动更新前端 dist 并重编译壳。
- **联机房间 / 开箱等玩法**：依赖官方服务端可用性，与官方 exe 端同源同命运。
- **键码映射**：小键盘 / JIS 键盘映射按 BongoCat 表校准，未在全键盘布局实测。
- **dmg 未公证**：adhoc 签名，他人下载首次打开需右键 → 打开。
- **前端版本绑定**：壳版本严格绑定 `frontend/` 中的 dist 版本；官方更新前端后需同步逆向提取新 dist 并重新对齐命令签名。

---

## 版本历史

| 版本 | 日期 | 内容 |
|---|---|---|
| v0.0.1-Beta | 2026-10-28 | 首个开源版本：壳重建完成；修复 hit_region 字段解析（left/top）、KrSDK https 加载、计数持久化、WKWebView keep-alive、面板 1240×800 适配；修复退出时键盘重复刷屏（事件转发改为异步 channel） |

---

## 贡献指南

当前 Beta 阶段，欢迎：

- 反馈 Bug 与功能建议（Issues）
- 贡献 macOS 权限 / 签名 / Tauri 相关修复
- 补充逆向分析中的命令签名或窗口模型细节

代码风格遵循 `cargo fmt` + `cargo clippy` 默认规则。**请勿改动二进制产物**（`frontend/assets/*.js` 等），前端补丁应在逆向提取时最小化。

---

## License

本仓库**未包含 License 文件**，也未在源文件中声明许可证。若你计划二次分发或商业使用，请联系项目维护者确认。

致谢代码来源的开源项目：
- [BongoCat](https://github.com/ayangweb/BongoCat)（Apache-2.0）：macOS CGEventTap 挂法与键码映射表参考

---

## 致谢

- **库洛游戏（Kuro Games）**：WuwaTappo 本体与云端服务
- [BongoCat](https://github.com/ayangweb/BongoCat)（Apache-2.0）：macOS CGEventTap 挂法（HIDEventTap + HeadInsertEventTap）与键码 → HID usage 映射表参考

---

## 免责声明

本项目为社区移植，与库洛游戏 / Kuro Games 无关。鸣潮（Wuthering Waves）及相关资产版权归其所有者所有。

本项目**不破解、不代理、不重实现任何官方服务**——仅复用官方公开客户端资产并直连官方服务端，对官方 API 不做任何逆向修改。

本项目仅供个人学习研究使用；若官方提出异议，本项目将立即下架处理。
