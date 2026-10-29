# WuwaTappo-macos Code Wiki

> 鸣潮桌宠 macOS 移植版 — 社区逆向重建壳  
> 仓库版本：v0.0.1-Beta | Tauri 2.10.3 | Rust Edition 2021  
> 官方前端资产版本：1.0.1（`Cargo.toml`/`tauri.conf.json` 中的版本号沿袭官方 exe 资产，便于对应官方功能集）  
> 仓库：https://github.com/EchoAI-01/WuwaTappo-macos  
> 文档更新：2026-10-29（同步 input.rs 双模式 tap / 通道化回调 / 几何缓存 / 固定签名等改动）

---

## 目录

- [1. 项目概述](#1-项目概述)
- [2. 架构总览](#2-架构总览)
  - [2.1 技术栈](#21-技术栈)
  - [2.2 系统架构图](#22-系统架构图)
  - [2.3 运行时数据流](#23-运行时数据流)
- [3. 目录结构](#3-目录结构)
- [4. 核心模块详解](#4-核心模块详解)
  - [4.1 应用入口与生命周期 (`main.rs` + `lib.rs::run`)](#41-应用入口与生命周期-mainrs--librsrun)
  - [4.2 全局输入监听模块 (`input.rs`)](#42-全局输入监听模块-inputrs)
  - [4.3 窗口管理与多窗口架构](#43-窗口管理与多窗口架构)
  - [4.4 托盘与右键菜单系统](#44-托盘与右键菜单系统)
  - [4.5 前端-后端命令桥（Invoke Handlers）](#45-前端-后端命令桥invoke-handlers)
  - [4.6 本地存储层](#46-本地存储层)
- [5. 关键类、结构体与函数索引](#5-关键类结构体与函数索引)
- [6. 依赖关系详解](#6-依赖关系详解)
- [7. 配置文件说明](#7-配置文件说明)
- [8. 项目运行与构建](#8-项目运行与构建)
- [9. 架构决策与已知限制](#9-架构决策与已知限制)
  - [9.1 架构决策记录](#91-架构决策记录)
  - [9.2 开发者必读：macOS 移植踩坑记录](#92-开发者必读macos-移植踩坑记录改相关代码前必读)
  - [9.3 Win32 → macOS 降级映射](#93-win32--macos-降级映射)
  - [9.4 已知限制](#94-已知限制)
  - [9.5 源码中的 no-op 命令](#95-源码中的-no-op-命令)
- [10. 事件协议参考](#10-事件协议参考)

---

## 1. 项目概述

**WuwaTappo-macos** 是库洛游戏（Kuro Games）旗下《鸣潮》（Wuthering Waves）桌宠应用 **WuwaTappo** 的 macOS 社区移植版。

官方只发布了 Windows 版（exe），本项目通过逆向 `WuwaTappo-cn_1.0.1_x64-setup.exe`，**完整复用官方前端 dist 与云端协议，为 macOS 重建原生外壳**，让 Mac 用户也能使用鸣潮桌宠，并与 Windows exe 端玩家正常联机互动。

### 移植策略（为什么是"复用前端 + 重建壳"）

曾对比三条路线：A. 虚拟机/兼容层跑 exe；B. **复用官方前端 dist，为 macOS 重建 Rust 原生壳**；C. 参照 BongoCat 用原生技术重写 UI。最终选定 **方案 B**，理由：

- 官方前端（Vue 3 + Vite）承载了几乎全部业务逻辑：皮肤系统（260 皮肤 × 分层 PNG）、开箱 / 合成 / 房间玩法、KuroSDK 登录、多语言。重写不可维护，且无法跟随官方更新。
- 官方后端是 Tauri 2（Rust），其自定义命令签名在未 strip 的二进制里可直接读取，按官方签名 1:1 重建即可与前端无缝对接，重建面远小于预估。
- 云端协议（REST + JSON）与平台无关，macOS 直连即可与 exe 端互通。

### 逆向工作流

1. `7zz` 双层解开 Tauri 安装壳 + 内嵌 NSIS 包，取出 `WuwaTappo.exe`（77.9 MB）与前端 dist；
2. 分析未 strip 的 Rust 符号，还原 **50 个自定义 invoke 命令** 与窗口模型；
3. 实测官方协议链路（配置发现 → 资源 CDN → 登录 → 玩法 API），确认 macOS 可直连；
4. 为 macOS 重建壳（本仓库 `src-tauri/`），对齐官方关键架构决策（全屏 shell 恒穿透 + 输入事件桥）；
5. 对官方前端 dist 做**最小限度补丁**（见下表），其余字节不动。

### 前端补丁清单（对官方 dist 的全部修改）

官方 dist 除以下补丁外字节不动：

| 文件 | 补丁 | 原因 |
|------|------|------|
| `assets/index-D8GJxFG7.js` | 应用显示文案 | 应用名适配 |
| | `isdkSrc` / `isdkCss` 由 `//pro-cdn-sdk.kurogame.com/...` 改为 `https://...` | protocol-relative URL 在 `tauri://localhost` 下解析失败，KuroSDK 加载不了，登录窗口空白（Windows 的 `https://tauri.localhost` 不受影响） |
| `assets/index-DmC0wUEw.js` | `key-press` 监听注册后追加 `notify_input_bridge_ready()` 注入 | 输入桥握手观测点（见踩坑 §3） |
| `assets/index-DenkafYz.js` | 文案适配 | 同应用名适配 |

### 设计目标

| 目标 | 说明 |
|------|------|
| **与 exe 端联机互通** | 直接对接官方服务端 API 与官方 KuroSDK 登录（非私服、非协议破解） |
| **保留官方全部功能** | 桌宠展示 / 换装 / 表情 / 开箱 / 合成 / 兑换码 / 房间 / 计数统计 |
| **平台原生** | 托盘、菜单、窗口动画使用 macOS 原生 API（NSMenu / NSWindow） |
| **无后台** | 桌宠窗口不占 Dock（`LSUIElement=true`），仅保留托盘入口 |

### 版本与标识

| 属性 | 值 |
|------|-----|
| 仓库版本 | v0.0.1-Beta |
| 官方前端资产版本 | 1.0.1（沿袭官方 exe，便于对应官方功能集） |
| 包名 | `wuwatappo-macos` |
| 产物名 | `WuwaTappo.app` |
| Bundle ID | `com.kuro.wuwatappo.cn` |
| 最低系统版本 | macOS 10.15 (Catalina) |

---

## 2. 架构总览

### 2.1 技术栈

```
┌─────────────────────────────────────────────────────────────────┐
│                  官方云服务（Kuro）                               │
│  配置发现 / 资源CDN / KuroSDK登录 / 玩法API / 房间                │
│                  REST + JSON（平台无关，与 exe 端同一协议）         │
└────────────────────────────────▲────────────────────────────────┘
                                 │
┌────────────────────────────────┴────────────────────────────────┐
│                     WuwaTappo.app (Tauri 2)                      │
├──────────────────────────┬──────────────────────────────────────┤
│     前端层 (WASM/JS)      │            原生层 (Rust)             │
├──────────────────────────┼──────────────────────────────────────┤
│ 官方 Vue SPA dist         │  Tauri Runtime (tauri ^2.10.3)      │
│ （编译期嵌入二进制）       │  CGEventTap (objc2-core-graphics)    │
│ WKWebView 渲染引擎        │  NSMenu / Tray Icon (Muda)          │
│ Hash 路由: #/桌宠         │  窗口管理 (WebviewWindowBuilder)    │
│            #/panel        │  自动启动 (tauri-plugin-autostart)  │
│            #/login        │  应用更新 (tauri-plugin-updater)    │
│            #/update       │  文件对话框 (tauri-plugin-dialog)   │
│            #/about        │  深度链接 (tauri-plugin-deep-link)  │
│            #/oss-notice   │  keep-alive 线程（防 idle 挂起）      │
├──────────────────────────┴──────────────────────────────────────┤
│                     系统 API 层                                  │
├─────────────────────────────────────────────────────────────────┤
│  CoreGraphics (CGEventTap) │ CoreFoundation (CFRunLoop)         │
│  ApplicationServices (辅助功能授权) │ LaunchServices (LaunchAgent)│
└─────────────────────────────────────────────────────────────────┘
```

### 2.2 系统架构图

```
┌─────────────────────────────────────────────────────────────────────┐
│  官方云服务（Kuro）                                                  │
│  REST + JSON（配置发现 / 资源CDN / KuroSDK登录 / 玩法API / 房间）     │
│  与 exe 端同一协议，联机互通                                         │
└────────────────────────────────▲────────────────────────────────────┘
                                 │
┌────────────────────────────────┴────────────────────────────────────┐
│                        用户操作层                                    │
│  鼠标移动 ──▶ CGEventTap ──▶ mouse-move ──▶ 桌宠动画               │
│  键盘敲击 ──▶ CGEventTap ──▶ key-press   ──▶ 打字计数             │
│  鼠标点击 ──▶ CGEventTap ──▶ mouse-button ──▶ 点击/拖拽判定         │
└─────────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                   Rust 原生输入桥 (input.rs)                        │
│  ┌─────────────────┐  ┌──────────────────┐  ┌───────────────────┐  │
│  │ CGEventTap      │  │ 命中测试          │  │ 事件直转无门控     │  │
│  │ HIDEventTap +   │──▶│ hit_test()       │──▶│ （握手仅日志观测） │  │
│  │ HeadInsert      │  │ 读几何缓存        │  │                   │  │
│  │                 │  │ try_lock 不阻塞   │  │ 避免初始化循环等   │  │
│  │ Default（首选） │  │ - main 窗口内     │  │ 待（踩坑 §3）      │  │
│  │  ↳ 命中吞事件   │  │ - 不在辅助窗口上  │  │                   │  │
│  │ ListenOnly      │  │ - 命中桌宠区域    │  │ 通道化：回调只    │  │
│  │  ↳ 悬停切穿透   │  │                   │  │ try_send，独立线程 │  │
│  └─────────────────┘  └──────────────────┘  │ emit（踩坑 §10）   │  │
│                                             └───────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
                                │ Tauri Event Bridge
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     WKWebView (main 全屏 shell)                      │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                   官方 Vue SPA Application                   │    │
│  │  ┌────────────┐ ┌────────────┐ ┌─────────────────────────┐  │    │
│  │  │ 桌宠渲染    │ │ 状态管理    │ │ 事件处理合成             │  │    │
│  │  │ 全屏透明    │ │ Pinia      │ │ elementFromPoint + click │  │    │
│  │  │ CSS 区域    │ │             │ │ beginDockPointerAt 手势 │  │    │
│  │  │ 2560×1440   │ │             │ │ fc() → 右键菜单 invoke │  │    │
│  │  └────────────┘ └────────────┘ └─────────────────────────┘  │    │
│  │                                                             │    │
│  │  桌宠交互区域 → invoke("set_main_hit_region", ...)           │    │
│  │  面板窗口打开 → invoke("open_panel_window", ...)             │    │
│  │  计数持久化   → invoke("save_key_press_count", ...)          │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
                                │ Tauri IPC (JSON)
                                ▼
┌─────────────────────────────────────────────────────────────────────┐
│                   Rust 命令处理器 (lib.rs)                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐  │
│  │ 50 个命令 │ │ 窗口管理  │ │ 托盘菜单  │ │ 本地存储  │ │ 更新/日志 │  │
│  │ (按官方   │ │           │ │           │ │           │ │          │  │
│  │  签名1:1 │ │           │ │           │ │           │ │          │  │
│  │  重建)    │ │           │ │           │ │           │ │          │  │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘ └──────────┘  │
└─────────────────────────────────────────────────────────────────────┘
                                │
                ┌───────────────┼───────────────┐
                ▼               ▼               ▼
        ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
        │ Tray Icon    │ │ AppLocalData│ │ 多窗口      │
        │ (NSMenu,     │ │ store.json  │ │ panel/login │
        │  多语言菜单) │ │ logs/       │ │ /about/...  │
        └─────────────┘ └─────────────┘ └─────────────┘
```

### 2.3 运行时数据流

#### 输入事件流

```
用户按下键盘 ─▶ macOS CGEventTap ─▶ input.rs event_tap_callback
                                      │
                                      ▼
                              map_key_code() 键码映射
                              + pressed_keys 位图更新
                                      │
                                      ▼
                              ctx.tx.try_send(TapMsg::Key)   ← 回调内不阻塞
                                      │
                                      ▼
                              input-emitter 线程 recv
                                      │
                                      ▼
                              emit_to("main", "key-press", key)
                                      │
                                      ▼
                              WebView main Vue SPA 监听 key-press
                                      │
                                      ▼
                              更新打字计数 / 桌宠响应
```

> 全局键盘事件需要「输入监控」权限；只有辅助功能时 tap 为 ListenOnly，收不到 KeyUp/KeyDown，此时只有 main 窗口聚焦时的原生 keydown 会计数。

#### 窗口创建流（以 panel 为例）

```
托盘左键点击 ─▶ tray.on_tray_icon_event ─▶ emit("dock-open-panel", "bag")
                                               │
                                               ▼
                              Vue SPA 监听 dock-open-panel
                                               │
                                               ▼
                              invoke("open_panel_window", { tab: "bag" })
                                               │
                                               ▼
                              Rust open_panel_window()
                                               │
                                               ▼
                              create_aux_window_min(app, "panel", "/panel", ...)
                                               │
                                               ▼
                              新 WebviewWindowBuilder 建立 panel 窗口
                              (URL: index.html#/panel)
```

#### 桌宠拖拽流

```
CGEventTap mouse-move ─▶ emit_to("main", "mouse-move", {x, y})
                                    │
                                    ▼
                        Vue SPA mouse-move 监听
                                    │
                                    ▼
                        (x - window.innerPosition) / scaleFactor → CSS 坐标
                                    │
                                    ▼
                        桌宠跟随鼠标移动 (CSS transform)
```

---

## 3. 目录结构

```
WuwaTappo-macos/
├── CODE_WIKI.md                    # 本文档
├── README.md                       # 项目说明（简短版）
├── .gitignore
│
├── frontend/                       # 前端编译产物（dist，原样复用）
│   ├── index.html                  # SPA 入口，#/ 路由
│   └── assets/                     # 编译后的 JS/CSS/图片/字体
│       ├── index-*.js               # 主应用 bundle
│       ├── index-*.css              # 样式
│       ├── *.woff2                  # 多语言字体
│       └── *.png, *.jpg             # UI 资源（桌宠精灵、按钮、对话框等）
│
└── src-tauri/                      # Rust/Tauri 原生壳
    ├── Cargo.toml                  # Rust 依赖清单
    ├── Cargo.lock                  # 依赖版本锁定
    ├── tauri.conf.json             # Tauri 应用配置（窗口、bundle 等）
    ├── Info.plist                  # macOS Bundle 配置
    ├── build.rs                    # Tauri Build 脚本
    │
    ├── capabilities/
    │   └── main.json               # Tauri 2 ACL 权限声明（173 条权限）
    │
    ├── gen/schemas/                # Tauri 自动生成的 JSON Schema
    │   ├── acl-manifests.json
    │   ├── capabilities.json
    │   ├── desktop-schema.json
    │   └── macOS-schema.json
    │
    ├── icons/                      # 应用图标（多尺寸）
    │   ├── icon.icns               # macOS .icns
    │   ├── icon.ico                # Windows .ico
    │   ├── *.png                   # 多分辨率 PNG
    │   └── ios/                    # iOS 图标（Tauri 标准结构）
    │
    └── src/                        # Rust 源码
        ├── main.rs                 # 可执行入口（main → lib::run）
        ├── lib.rs                  # Tauri 应用主逻辑（1334 行）
        └── input.rs                # 全局输入监听模块（539 行）
```

---

## 4. 核心模块详解

### 4.1 应用入口与生命周期 (`main.rs` + `lib.rs::run`)

#### 文件位置

- [main.rs](file:///Users/chen/AI/桌宠/WuwaTappo-macos/src-tauri/src/main.rs) (6 行)
- [lib.rs](file:///Users/chen/AI/桌宠/WuwaTappo-macos/src-tauri/src/lib.rs) (1334 行)

#### 启动流程

```
main()
  └── wuwatappo_macos_lib::run()
        └── tauri::Builder::default()
              ├── .plugin(tauri_plugin_opener::init())
              ├── .plugin(tauri_plugin_process::init())
              ├── .plugin(tauri_plugin_dialog::init())
              ├── .plugin(tauri_plugin_deep_link::init())
              ├── .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent))
              ├── .plugin(tauri_plugin_updater::Builder::new().build())
              │
              ├── .setup(|app| { ... })   ← 应用初始化钩子
              │     ├── manage(SharedTrayState)
              │     ├── manage(SharedMainHitState)
              │     ├── manage(InputBridgeReady)
              │     ├── 创建 Tray Icon
              │     ├── 重建托盘菜单（rebuild_tray_menu）
              │     ├── main 窗口 set_visible_on_all_workspaces(true)
              │     ├── input::start()   ← 启动 CGEventTap 线程
              │     └── 启动 keep-alive + 诊断线程（每 5s eval("void 0")）
              │
              ├── .on_page_load(...)   ← 页面加载完成钩子
              │     ├── 注入前端诊断脚本（onerror/unhandledrejection 捕获 + 10s 心跳）
              │     └── 屏蔽所有窗口右键菜单（contextmenu preventDefault）
              │
              ├── .invoke_handler(tauri::generate_handler![...40+ 命令...])
              │
              └── .run(tauri::generate_context!())
                    └── 进入 Tauri 主消息循环
```

#### Keep-Alive 机制

WKWebView 对"无活动"页面会执行 idle 挂起（JS 冻结、timer 停转）。项目通过后台线程每 5 秒对所有 WebView 注入 `void 0` eval 来维持页面活跃。前 2 分钟同时输出窗口/状态诊断日志到 `/tmp/wuwatappo-diag.log`，之后仅保留 keep-alive。

### 4.2 全局输入监听模块 (`input.rs`)

#### 文件位置

- [input.rs](file:///Users/chen/AI/桌宠/WuwaTappo-macos/src-tauri/src/input.rs) (864 行)

#### 核心职责

| 职责 | 实现 |
|------|------|
| 全局键鼠事件采集 | `CGEventTap`（HIDEventTap + HeadInsertEventTap），**Default / ListenOnly 双模式** |
| 阻止点击穿透 | Default 模式下对桌宠区域内的 press/drag/up 返回 `null` 吞掉事件 |
| 键码映射 | macOS virtual keycode → `KeyboardEvent.key` 风格字符串（覆盖 104 键 + 小键盘 + 功能键） |
| 事件节流 | mouse-move 最小 16ms 间隔（约 60fps） |
| 命中区域过滤 | `hit_test()` per-region 判断（纯内存，读几何缓存） |
| 双权限处理 | 辅助功能（`AXIsProcessTrusted`）+ 输入监控（`CGPreflightListenEventAccess` / `CGRequestListenEventAccess`） |
| tap 超时恢复 | runloop 分片运行，`TapDisabledByTimeout` 自动 `tap_enable` 并**补发按键 release** |
| 降级防穿透 | 无输入监控权限时，指针悬停桌宠即让 main 窗口接收鼠标（`apply_pointer_hover`） |

#### 两种 tap 模式（关键设计）

macOS 对 CGEventTap 的权限要求分两级，必须区分：

| 模式 | 权限要求 | 能力 | 本移植中的用途 |
|------|----------|------|----------------|
| `CGEventTapOptions::Default` | 辅助功能 **+ 输入监控** | 可修改/吞掉事件（回调返回 `null`） | **首选**：桌宠区域点击不穿透桌面 |
| `CGEventTapOptions::ListenOnly` | 仅辅助功能 | 只能观测，事件必然透传 | **降级**：点击会穿透到桌面 |

启动逻辑（`run_worker`）：

```
CGPreflightListenEventAccess() == false
  → 打开「系统设置 → 隐私与安全性 → 输入监控」
  → 调 CGRequestListenEventAccess() 触发系统授权弹窗
  → 先建 ListenOnly（保证鼠标事件能进来）
  → 每 2s 复查权限，一旦授权立即 break 重建 Default tap（无需重启 App）

CGPreflightListenEventAccess() == true
  → 直接建 Default tap；失败则回退 ListenOnly
```

> **签名与授权持久性**：adhoc 签名（`signingIdentity: "-"`）每次编译 cdhash 都变，macOS TCC 会把新二进制当成新应用，之前的授权自动失效——这正是"给了权限却没用、过一会儿又失效"的机制性原因。发布版必须用**固定证书**（本机已配置 `WuwaTappo Dev Signing`）签名，授权才能跨编译保持。

#### 事件协议

```rust
// key-press / key-release：payload 为字符串
emit_to("main", "key-press", "W")
emit_to("main", "key-release", "ArrowUp")

// mouse-move：payload 为 {x, y}，全局物理像素
emit_to("main", "mouse-move", {"x": 1440.0, "y": 900.0})

// mouse-button：payload 为 {button, action}
emit_to("main", "mouse-button", {"button": "left", "action": "press"})
emit_to("main", "mouse-button", {"button": "right", "action": "release"})
```

#### 线程模型（零阻塞回调）

```
主线程 (Tauri)
  ├── 窗口几何查询 / 菜单 / 托盘
  ├── set_main_hit_region → refresh_main_geom（100ms 节流刷新几何缓存）
  └── apply_pointer_hover（ListenOnly 降级时的穿透切换）

global-input 线程 (worker)
  ├── 权限探测与请求（辅助功能 / 输入监控）
  ├── 建立 CGEventTap
  ├── CFRunLoop::run_in_mode（分片 50ms）
  │     └── event_tap_callback（必须微秒级返回）
  │           ├── KeyDown/KeyUp → 维护 pressed_keys 位图 → try_send(TapMsg::Key)
  │           ├── FlagsChanged → handle_flags_changed → try_send(TapMsg::Key)
  │           ├── MouseMoved/Dragged → 节流 → try_send(TapMsg::Move)
  │           ├── MouseDown → hit_test → try_send(TapMsg::Button) → 命中则 return null 吞掉
  │           ├── MouseUp → try_send(TapMsg::Button) → 若本次按下属吞状态则 return null
  │           └── TapDisabled → needs_restart = true
  └── 分片循环：needs_restart → 补发 stale key-release → tap_enable(true)

input-emitter 线程（独立）
  └── 从 sync_channel(1024) 取 TapMsg → emit_to 前端 / 投递 Hover 到主线程
```

**为什么必须通道化**：CGEventTap 回调超过系统阈值（约 0.25s）时，**该事件会被系统直接丢弃**。一旦 KeyUp 被丢，系统认为按键一直按着 → 触发键盘重复，表现为按 Cmd+Q 退出时字符疯狂刷屏。回调里任何 `emit_to` / 写盘 / `Mutex::lock` 都可能触发，因此全部移到独立线程，回调只做内存判断与 `try_send`（队列满则丢帧，绝不阻塞）。

#### 命中区域过滤算法 (`hit_test`)

```
输入：CG 全局逻辑坐标 (gx, gy)
  1. 读 SharedMainGeom 缓存（try_lock，避免回调被主线程持锁阻塞）
     └── main 是否可见、main 的全局矩形 (x, y, w, h)
  2. 点是否在 main 矩形内？否 → false
  3. 点是否落在辅助窗口（panel/login/toast 等）矩形内？是 → false
     （这些点击属于辅助窗口自己）
  4. 转窗口内 CSS 逻辑坐标：lx = gx - mx, ly = gy - my
     （CG 逻辑点与 Tauri outer_position 同为逻辑坐标，直接相减，不做 scale 换算）
  5. 命中 input_regions 或 visual_regions → true
```

**几何缓存（`MainGeom` / `SharedMainGeom`）**：tap 回调运行在独立线程，而 Tauri/tao 的窗口查询（`get_webview_window` / `outer_position` / `is_visible`）必须在主线程调用。直接在回调里查窗口 API 会导致进程不定时静默崩溃。因此命中测试只读这份由主线程（`refresh_main_geom`）维护的快照，缓存字段包含 main 几何、辅助窗口列表、前端 force-ignore 期望、指针是否在桌宠内。

#### 按下键位图（`pressed_keys`）

`TapContext.pressed_keys: AtomicU64` 记录 keycode < 64 的按键按下状态（位图）。用途：

- tap 被系统禁用 → 重启时 `swap(0)` 取出所有"仍按下"的键，逐个补发 `key-release`
- 前端 `index-DmC0wUEw.js` 的 `_c()/kc()` 用 `Pr` Set 做去重：**keyup 一旦丢失，该键会永久卡在 Set 里，表现为"计数器用一会儿就彻底不计数"**
- 补发 release 是清理这一残留的唯一手段

#### 降级防穿透（`apply_pointer_hover`）

当只能建 ListenOnly（无输入监控权限）时，无法吞事件，因此改为**窗口级动态穿透**：

| 指针位置 | `main.set_ignore_cursor_events` | 效果 |
|----------|-------------------------------|------|
| 在桌宠区域内 | `false`（接收） | 点击被窗口接住，不落到桌面 |
| 在桌宠区域外 | 由 `front_force_ignore` 决定（默认 `true` 穿透） | 桌面正常使用 |

切换只在跨越区域边界时发生（`pointer_in_pet` 状态变化才入队 `TapMsg::Hover`）。Default 模式下**不启用**该逻辑——窗口恒穿透，由 tap 吞事件，避免 WKWebView 原生 DOM 事件与前端合成事件冲突。


### 4.3 窗口管理与多窗口架构

#### 窗口列表

| Label | 用途 | URL 路由 | 尺寸 (W×H) | 属性 |
|-------|------|----------|------------|------|
| `main` | 桌宠主窗口（全屏透明 shell） | `index.html` (根) | **tauri.conf.json 初始 320×420 → 前端启动后 fallback 为 2560×1440 全屏** | transparent, decorations=false, alwaysOnTop=true, skipTaskbar=true, ignoresMouseEvents **恒穿透** |
| `toast` | Toast 提示 | `index.html` | 420×120 | transparent, alwaysOnTop=true, skipTaskbar=true |
| `panel` | 背包/图鉴/兑换/房间/设置 | `index.html#/panel` | 1240×800 (min 同) | transparent, decorations=false, resizable=true, shadow=true, 不穿透 |
| `login` | 登录窗口 | `index.html#/login` | **520×440**（前端卡片固定 500×378，窗口必须容得下） | transparent, decorations=false, resizable=false, always_on_top=true, 不穿透 |
| `update` | 更新窗口 | `index.html#/update` | 420×560 | transparent, decorations=false, resizable=false, 不穿透 |
| `about` | 关于窗口 | `index.html#/about` | 420×520 | transparent, decorations=false, resizable=false, 不穿透 |
| `oss-notice` | OSS 许可 | `index.html#/oss-notice` | 460×600 | transparent, decorations=false, resizable=true, 不穿透 |

> **关键架构**：main 窗口是 **全屏透明 shell**（覆盖整个屏幕），`ignoresMouseEvents` 恒穿透。桌宠本体绘制在右下角 CSS 区域（绝对定位）。所有辅助窗口不穿透，正常接收点击。全部窗口共用同一个 `index.html`，靠 hash 路由区分页面。

#### 交互管线（本移植的核心）

官方 Windows 版用 Win32 per-region 命中测试（`WM_NCHITTEST`）实现"桌宠可点、空白处穿透"。macOS 没有 per-region 窗口级命中测试，因此按权限分两条路径：

**路径 A（首选，需「输入监控」权限）：Default tap 吞事件**

```
main 窗口恒穿透（ignoresMouseEvents = true）
CGEventTap（Default，可修改事件）
  │ MouseDown → hit_test()
  │   命中桌宠区域 → emit mouse-button(press) → return null（吞掉，桌面收不到）
  │   未命中       → emit mouse-button(press) → 原样透传
  │ MouseUp/Dragged → 若本次按下属吞状态 → return null
  │ KeyDown/KeyUp   → 永远不吞（吞 KeyUp 会触发系统按键重复）
  ▼
前端按 mouse-move 坐标 + elementFromPoint 合成点击/拖拽/右键
```

**路径 B（降级，仅「辅助功能」权限）：窗口动态穿透**

```
main 窗口初始穿透
Pointer 进入桌宠区域 → set_ignore_cursor_events(false)（窗口开始接收鼠标）
Pointer 离开桌宠区域 → set_ignore_cursor_events(true)（恢复穿透）
  ▼
点击被窗口接住（不落到桌面），前端仍靠合成事件处理交互
```

两条路径共用同一套前端事件协议：

```
CGEventTap
  │ mouse-move（物理像素，约 60fps 节流）
  │ mouse-button（press/release × left/right）
  │ key-press / key-release（KeyboardEvent.key 风格字符串）
  ▼
emit → main 窗口前端
  │ elementFromPoint + click()      → 合成按钮点击（引导按钮/宝箱/设置）
  │ beginDockPointerAt 等手势函数    → 合成拖拽
  │ fc()                            → 右键 → invoke popup_main_dock_context_menu
  ▼
前端把桌宠可交互区域经 set_main_hit_region 上报 Rust
  （inputRegions + regions，字段 left/top/width/height）
  → 每次上报顺带刷新 SharedMainGeom 几何缓存（100ms 节流）
```

**坐标换算**：CGEvent location 是全局逻辑点；`hit_test` 直接与 Tauri `outer_position`（同为逻辑坐标）相减得到窗口内 CSS 坐标。而 `mouse-move` 的 payload 需要**物理像素**（前端 `Ht()` 用 `(x − innerPosition) / scaleFactor` 还原），因此 emit 前按鼠标所在显示器 backing scale 乘算。

#### main 与辅助窗口的层级关系（`sync_main_layer`）

`main` 配置为 `alwaysOnTop: true` 且覆盖全屏，与 login/panel 同属 floating level。macOS 上同层窗口谁最后 `orderFront` 谁在上面，`main` 只要被激活就会盖住辅助窗口——实测出现过 `login: visible=true` 但用户完全看不见的情况。

`sync_main_layer(app)` 的处理：

```rust
// 只要有可见的辅助窗口，就把 main 降为非置顶（normal level）
let has_aux = webview_windows().iter()
    .any(|(label, w)| label != "main" && w.is_visible().unwrap_or(false));
main.set_always_on_top(!has_aux);
```

调用时机：`create_aux_window_min` 创建成功后、以及每个 `close_*_window` 关闭后。辅助窗口全部关闭时 main 恢复置顶，桌宠继续常在最前。

#### 创建函数

```rust
// 通用辅助窗口创建
fn create_aux_window<R: Runtime>(
    app: &AppHandle<R>,
    label: &str,        // 窗口唯一标识
    route: &str,         // Hash 路由（#/panel → "/panel"）
    title: &str,         // 窗口标题
    w: f64,              // 初始宽度
    h: f64,              // 初始高度
    resizable: bool,     // 是否可调整尺寸
) -> tauri::Result<()>

// 带最小尺寸版本
fn create_aux_window_min<R: Runtime>(..., min_w: f64, min_h: f64) -> tauri::Result<()>
```

#### 主窗口特殊设置

- **穿透与命中**：`set_main_force_cursor_ignore(enabled)` 不再直接透传参数，而是
  `set_ignore_cursor_events(enabled && !pointer_in_pet)` —— 记录前端"希望穿透"的期望（`front_force_ignore`），但只要指针悬停在桌宠区域内就强制保持可点击，否则点击会落到桌面
- **多工作区可见**：`set_visible_on_all_workspaces(true)` — 桌宠跨 Space 显示
- **Dock 隐藏**：`LSUIElement=true` (Info.plist) — 不占 Dock 图标
- **几何缓存**：`set_main_hit_region` / `set_main_window_content_bounds` / `sync_main_window_to_current_monitor` / `set_main_force_cursor_ignore` 等布局相关命令末尾都会调用 `refresh_main_geom`，把最新窗口外框写入 tap 线程可读的快照

### 4.4 托盘与右键菜单系统

#### 状态结构体

```rust
// 托盘状态（sync_native_menu 持续同步）
pub struct TrayState {
    labels: MenuLabels,          // 多语言菜单文案
    pet_visible: bool,           // 桌宠可见性
    pet_scale: f64,              // 缩放百分比 (50-200)
    window_opacity: f64,         // 透明度百分比 (20,40,60,80,100)
    window_passthrough: bool,    // 是否点击穿透
}

// 托盘状态托管（Tauri State）
pub struct SharedTrayState(pub Mutex<TrayState>);
```

#### 菜单结构

```
┌─ 托盘/右键菜单 ─────────────────────────────────────┐
│  📦 打开背包        ──▶ emit("dock-open-panel", "bag") │
│  🗺️  图鉴          ──▶ emit("dock-open-panel", "atlas")│
│  💱 兑换            ──▶ emit("dock-open-panel", "...") │
│  🏠 联机房间        ──▶ emit("dock-open-panel", "...") │
├─────────────────────────────────────────────────────┤
│  [隐藏/显示桌宠]     ──▶ emit("tray-toggle-pet-visibility") │
│  ▶ 桌宠大小 ─────┐                                    │
│    ├☐ 50%         │                                    │
│    ├☑ 100%        │                                    │
│    ├☐ 125%        │                                    │
│    ├☐ 150%        │                                    │
│    └...           │                                    │
│  ◀────────────────┘                                    │
│  ▶ 不透明度 ──────┐                                    │
│    ├☐ 20%         │                                    │
│    ├☑ 100%        │                                    │
│    └...           │                                    │
│  ☑ 点击穿透                                           │
├─────────────────────────────────────────────────────┤
│  ⚙️ 设置            ──▶ emit("dock-open-panel", "settings") │
│  🔄 重启应用        ──▶ emit("tray-restart")             │
│  ❌ 退出            ──▶ emit("tray-quit") (+ 3s 兜底强制退出)│
└─────────────────────────────────────────────────────┘
```

#### 事件处理流程

```
用户点击菜单项 (menu event id = "pet_scale_125.0")
  │
  ▼
handle_menu_event(app, id)
  │
  ├─ 修改 SharedTrayState（更新选中态）
  ├─ rebuild_tray_menu()（重建菜单勾选状态）
  │
  ▼
app.emit("tray-set-pet-scale", 125.0)
  │
  ▼
前端监听 tray-set-pet-scale
  ├─ 调用 set_main_window_content_bounds() 更新窗口尺寸
  ├─ 持久化设置
```

### 4.5 前端-后端命令桥（Invoke Handlers）

项目共注册 **40+ 自定义 Tauri 命令**，按功能分组：

#### 更新与遥测（7 个）

| 命令 | 签名 | 说明 |
|------|------|------|
| `check_app_update` | `async (endpoint: String) → Result<Option<Value>>` | 调用 Tauri updater 检查更新；服务器 darwin 目标缺失时返回 None |
| `get_track_os_info` | `() → Value` | 遥测系统信息（kurodata 埋点） |
| `is_force_update_locked` | `() → bool` | 强更锁（Windows 专属，macOS 恒 false） |
| `open_maintenance_notice` | `(payload: Option<Value>)` | emit `maintenance-notice-open` 给前端 |
| `show_toast_overlay` | `async () → Result<()>` | 创建/显示 toast 窗口（420×120，顶部居中） |
| `sync_native_menu` | `(labels: MenuLabels, pet_dock_visible: Option<bool>, ...)` | 同步多语言菜单文案到原生托盘 |
| `write_app_log` | `(level: String, message: String) → Result<()>` | 写入 `$APPLOCALDATA/logs/app-YYYYMMDD.log` |

#### 桌宠主窗口管理（11 个）

| 命令 | 说明 |
|------|------|
| `set_main_hit_region` | 前端上报交互/可视区域 → SharedMainHitState |
| `set_main_force_cursor_ignore` | 强制全窗光标穿透 |
| `set_main_swallow_mouse` | 鼠标吞噬（Win32 专属，macOS no-op） |
| `set_main_skip_taskbar` | 任务栏图标显隐 |
| `set_main_window_content_bounds` | 按前端 DOM 实测内容同步窗口几何 |
| `set_overlay_cursor` | 覆盖光标（Win32 专属，macOS no-op） |
| `sync_main_window_to_current_monitor` | 同步到鼠标所在显示器 |
| `popup_main_dock_context_menu` | 弹出桌宠右键菜单 |
| `get_boot_diagnostics` | 启动诊断信息 |
| `is_devtools_hotkeys_build` | 是否 devtools 快捷键构建（恒 false） |
| `is_internal_testing_build` | 是否内部测试构建（恒 false） |

#### 显示器/窗口布局（7 个）

| 命令 | 说明 |
|------|------|
| `apply_main_window_position` | 设置 alwaysOnTop + 可选落位 |
| `set_main_window_fullscreen_shell` | 全屏 shell 模式（Win32 专属，故意返回 Err 走 fallback） |
| `set_main_window_monitor_bounds` | 移动窗口到目标显示器坐标 |
| `get_main_desktop_safe_area` | 桌面安全区（Windows 任务栏避让，macOS 返回零） |
| `get_main_taskbar_insets` | 任务栏 insets（macOS 恒零） |
| `finish_main_window_monitor_switch` | 显示器切换收尾（macOS no-op） |
| `sync_aux_windows_to_main_monitor` | 辅助窗口跟随主窗口显示器 |

#### Update/维护公告（4 个）

| 命令 | 说明 |
|------|------|
| `get_app_update_detail` | 上次更新详情（macOS 无通道，恒 null） |
| `get_maintenance_notice_payload` | 维护公告内容（恒 null） |
| `has_maintenance_notice` | 是否有待展示公告（恒 false） |
| `hide_main_after_maintenance_notice_ready` | 公告就绪后隐藏主窗口 |

#### 辅助窗口管理（12 个）

| 命令族 | 命令 |
|--------|------|
| Panel | `open_panel_window`, `close_panel_window`, `raise_panel_window_above_pets`, `set_panel_window_collapsed` |
| Login | `open_login_window`, `close_login_window`, `resize_login_window_to_content` |
| Update | `open_update_window`, `close_update_window` |
| About | `open_about_window`, `close_about_window` |
| OSS Notice | `open_oss_notice_window`, `close_oss_notice_window` |

#### 自启动、存储、退出（7 个）

| 命令 | 说明 |
|------|------|
| `is_elevated_autostart_enabled` | 查询自启动状态 |
| `set_elevated_autostart` | 启用/禁用自启动（LaunchAgent） |
| `load_key_press_count` | 读取打字计数（返回裸数字 i64） |
| `save_key_press_count` | 保存打字计数 |
| `load_cumulative_click_count` | 读取累计点击计数 |
| `save_cumulative_click_count` | 保存累计点击计数 |
| `notify_input_bridge_ready` | 前端输入桥就绪握手（仅日志观测） |
| `exit_app` | 前端完成清理后的最终退出 |

### 4.6 本地存储层

#### 文件位置

`$APPLOCALDATA/native-store/store.json`

#### 数据结构

```json
{
  "key_press": {
    "count": 1234,
    "environment": "production"
  },
  "cumulative_click": {
    "count": 5678,
    "cuid": "user-id-string",
    "environment": "production"
  }
}
```

#### 存储函数

| 函数 | 签名 | 说明 |
|------|------|------|
| `store_path` | `(app: &AppHandle) → Result<PathBuf>` | 解析存储文件路径 |
| `store_read` | `(app: &AppHandle) → Value` | 读取 JSON（失败返回空对象） |
| `store_write` | `(app: &AppHandle, v: &Value)` | 写入 pretty JSON |

#### 日志文件

`$APPLOCALDATA/logs/app-YYYYMMDD.log`

```
[HH:MM:SS] [error] onerror: Failed to fetch @https://...:23
[HH:MM:SS] [info] heart beat #/panel ts=1730457600000
```

#### 诊断日志

| 文件 | 用途 |
|------|------|
| `/tmp/wuwatappo-diag.log` | 窗口/状态诊断（keep-alive 线程前 2 分钟双写） |
| `/tmp/wuwatappo-input.log` | 输入模块权限状态记录 |

---

## 5. 关键类、结构体与函数索引

### 5.1 lib.rs 核心类型

| 类型 | 位置 (行号) | 说明 |
|------|------------|------|
| `MenuLabels` | L27-57 | 多语言托盘文案结构体（14 个字段，带 serde default） |
| `TrayState` | L73-80 | 托盘运行时状态 |
| `SharedTrayState` | L81 | 托盘状态托管（Mutex<TrayState>） |
| `InputBridgeReady` | L978 | 输入桥就绪标志（Arc<AtomicBool>） |

### 5.2 input.rs 核心类型

| 类型 | 说明 |
|------|------|
| `RegionRect` | 窗口内 CSS 逻辑坐标矩形（含 `contains()`） |
| `MainHitState` | 命中区域集合（`input_regions` + `visual_regions`） |
| `SharedMainHitState` | 命中状态托管（`Mutex<MainHitState>`），前端 `set_main_hit_region` 写入 |
| `MainGeom` | 窗口几何快照：main 的 visible/x/y/w/h、辅助窗口列表 `aux`、`front_force_ignore`、`pointer_in_pet` |
| `SharedMainGeom` | 几何快照托管（`Mutex<MainGeom>`），tap 线程只读 |
| `TapMsg` | tap 回调 → 发送线程的消息枚举：`Key` / `Move` / `Button` / `Hover` |
| `TapContext` | CGEventTap 回调上下文：`app`、`last_move_ms`、`swallowing`、`pointer_in_pet`、`active_default`、`tx`、`pressed_keys`、`needs_tap_restart` |

### 5.3 关键函数

#### lib.rs 函数

| 函数 | 签名 | 说明 |
|------|------|------|
| `run` | `pub fn run()` | Tauri 应用主入口 |
| `rebuild_tray_menu` | `fn rebuild_tray_menu(app, st) → Result<()>` | 重建托盘菜单（按 TrayState） |
| `build_dock_menu` | `pub fn build_dock_menu(app, st) → Result<Menu>` | 构建桌宠菜单（托盘/右键共用） |
| `handle_menu_event` | `fn handle_menu_event(app, id)` | 菜单项事件分发 |
| `create_aux_window_min` | `fn create_aux_window_min(app, label, route, title, w, h, resizable, min_w, min_h) → Result<()>` | 带最小尺寸的辅助窗口创建（创建后 `sync_main_layer` + 刷新几何） |
| `sync_main_layer` | `fn sync_main_layer(app)` | 有辅助窗口时把 main 降为非置顶，全部关闭后恢复 |
| `open_panel` | `fn open_panel(app, tab) → Result<()>` | 打开 panel 并聚焦 tab |
| `dlog` | `pub fn dlog(msg: &str)` | 双写诊断日志（stdout + /tmp 文件） |
| `chrono_date` / `chrono_time` | `fn → String` | 自实现日期/时间格式化 |
| `store_path` / `store_read` / `store_write` | — | 本地 store.json 读写 |

#### input.rs 函数

| 函数 | 签名 | 说明 |
|------|------|------|
| `start` | `pub fn start(app: AppHandle)` | 启动全局输入监听（非阻塞，spawn worker） |
| `run_worker` | `fn run_worker(app: AppHandle)` | worker 主循环（权限探测 → tap 建立 → runloop 分片 → 超时重启/权限升级） |
| `event_tap_callback` | `unsafe extern "C-unwind" fn(...) → *mut CGEvent` | tap 回调：纯内存判断 + `try_send`，命中桌宠则返回 `null` 吞事件 |
| `hit_test` | `pub fn hit_test(app, gx, gy) → bool` | 命中测试（读几何缓存，`try_lock`） |
| `refresh_main_geom` | `pub fn refresh_main_geom(app)` | 主线程刷新几何缓存（100ms 节流） |
| `apply_pointer_hover` | `pub fn apply_pointer_hover(app, in_pet)` | ListenOnly 降级：按指针位置切换窗口穿透 |
| `spawn_emitter` | `fn spawn_emitter(app, rx)` | 独立发送线程：从 channel 取消息并 emit 给前端 |
| `update_pointer_mode` | `fn update_pointer_mode(ctx, gx, gy)` | 跨越桌宠边界时入队 `Hover`（Default 模式不启用） |
| `emit_key` | `fn emit_key(ctx, key, pressed)` | 入队 `TapMsg::Key` |
| `emit_mouse_button` | `fn emit_mouse_button(ctx, right, press)` | 入队 `TapMsg::Button` |
| `handle_mouse_button_now` | `pub fn handle_mouse_button_now(app, right, press)` | 直接 emit mouse-button（供非 tap 路径调用） |
| `mark_input_bridge_ready` | `pub fn mark_input_bridge_ready(app)` | 前端握手回调（仅日志观测） |
| `parse_regions` | `pub fn parse_regions(v) → Vec<RegionRect>` | 宽松解析前端上报的 regions（兼容 x/left、y/top） |
| `map_key_code` | `fn map_key_code(code) → Option<&'static str>` | macOS virtual keycode → KeyboardEvent.key |
| `handle_flags_changed` | `fn handle_flags_changed(ctx, code, flags)` | FlagsChanged 修饰键处理 |
| `to_physical` / `display_scale_at` | `fn → (f64, f64)` / `fn → f64` | CG 逻辑点 → 物理像素（查鼠标所在显示器 scale） |
| `listen_event_granted` | `fn listen_event_granted() → bool` | 查询「输入监控」权限（`CGPreflightListenEventAccess`） |
| `event_mask` | `fn event_mask() → CGEventMask` | 构造 CGEventTap 监听 mask |

### 5.4 lib.rs 命令处理器完整清单

> 共 50 个命令，按官方签名 1:1 重建。行号为编写时快照，改动后以函数名为准。

| # | 命令函数 | 行号 | 返回类型 | 分类 |
|---|---------|------|----------|------|
| 1 | `check_app_update` | L90 | `async Result<Option<Value>, String>` | 更新 |
| 2 | `get_track_os_info` | L124 | `Value` | 遥测 |
| 3 | `is_force_update_locked` | L135 | `bool` | 维护 |
| 4 | `open_maintenance_notice` | L142 | `()` | 维护 |
| 5 | `show_toast_overlay` | L151 | `async Result<(), String>` | Toast |
| 6 | `sync_native_menu` | L188 | `()` | 托盘 |
| 7 | `write_app_log` | L212 | `Result<(), String>` | 日志 |
| 8 | `set_main_hit_region` | L294 | `Result<(), String>` | 命中区域 |
| 9 | `set_main_force_cursor_ignore` | L327 | `Result<(), String>` | 穿透 |
| 10 | `set_main_swallow_mouse` | L337 | `()` | Win32 no-op |
| 11 | `set_main_skip_taskbar` | L344 | `Result<(), String>` | Dock |
| 12 | `set_main_window_content_bounds` | L354 | `Result<(), String>` | 窗口尺寸 |
| 13 | `set_overlay_cursor` | L391 | `()` | Win32 no-op |
| 14 | `sync_main_window_to_current_monitor` | L398 | `Result<(), String>` | 显示器同步 |
| 15 | `popup_main_dock_context_menu` | L430 | `Result<(), String>` | 右键菜单 |
| 16 | `get_boot_diagnostics` | L453 | `Value` | 诊断 |
| 17 | `is_devtools_hotkeys_build` | L471 | `bool` | 构建标记 |
| 18 | `is_internal_testing_build` | L478 | `bool` | 构建标记 |
| 19 | `apply_main_window_position` | L489 | `Result<(), String>` | 窗口位置 |
| 20 | `set_main_window_fullscreen_shell` | L526 | `Result<(), String>` | Win32 only |
| 21 | `set_main_window_monitor_bounds` | L533 | `Result<(), String>` | 显示器切换 |
| 22 | `get_main_desktop_safe_area` | L559 | `Value` | 安全区 |
| 23 | `get_main_taskbar_insets` | L569 | `Value` | 任务栏 |
| 24 | `finish_main_window_monitor_switch` | L576 | `()` | no-op |
| 25 | `sync_aux_windows_to_main_monitor` | L583 | `()` | 辅助窗口 |
| 26 | `get_app_update_detail` | L605 | `Option<Value>` | 更新详情 |
| 27 | `get_maintenance_notice_payload` | L612 | `Option<Value>` | 公告 |
| 28 | `has_maintenance_notice` | L619 | `bool` | 公告状态 |
| 29 | `hide_main_after_maintenance_notice_ready` | L626 | `()` | 公告后隐藏 |
| 30 | `open_panel_window` | L701 | `Result<(), String>` | 面板 |
| 31 | `close_panel_window` | L720 | `()` | 面板 |
| 32 | `raise_panel_window_above_pets` | L729 | `()` | 面板层级 |
| 33 | `set_panel_window_collapsed` | L739 | `Result<(), String>` | 面板折叠 |
| 34 | `open_login_window` | L754 | `Result<(), String>` | 登录 |
| 35 | `close_login_window` | L766 | `()` | 登录 |
| 36 | `resize_login_window_to_content` | L774 | `Result<(), String>` | 登录尺寸 |
| 37 | `open_update_window` | L789 | `Result<(), String>` | 更新窗口 |
| 38 | `close_update_window` | L804 | `()` | 更新窗口 |
| 39 | `open_about_window` | L812 | `Result<(), String>` | 关于 |
| 40 | `close_about_window` | L819 | `()` | 关于 |
| 41 | `open_oss_notice_window` | L827 | `Result<(), String>` | OSS |
| 42 | `close_oss_notice_window` | L834 | `()` | OSS |
| 43 | `is_elevated_autostart_enabled` | L846 | `Result<bool, String>` | 自启动 |
| 44 | `set_elevated_autostart` | L855 | `Result<(), String>` | 自启动 |
| 45 | `load_key_press_count` | L901 | `i64` | 打字计数 |
| 46 | `save_key_press_count` | L922 | `()` | 打字计数 |
| 47 | `load_cumulative_click_count` | L934 | `Value` | 累计点击 |
| 48 | `save_cumulative_click_count` | L959 | `()` | 累计点击 |
| 49 | `notify_input_bridge_ready` | L981 | `()` | 握手 |
| 50 | `exit_app` | L992 | `()` | 退出 |

---

## 6. 依赖关系详解

### 6.1 Cargo.toml 依赖清单

```toml
[package]
name = "wuwatappo-macos"
version = "1.0.1"
edition = "2021"

[lib]
crate-type = ["staticlib", "cdylib", "rlib"]   # 多产物（Rustlib/dylib/staticlib）

[build-dependencies]
tauri-build = { version = "2.5.0" }             # Tauri 构建脚本

[dependencies]
tauri = { version = "^2.10.3", features = [     # Tauri 核心
    "tray-icon",                                 # ✅ 托盘图标支持
    "image-png",                                 # ✅ PNG 图标
    "macos-private-api",                        # ✅ macOS 私有 API（LSUIElement 等）
    "protocol-asset",                            # ✅ asset:// 协议（资源访问）
] }

tauri-plugin-opener = "2"                        # 打开 URL/文件
tauri-plugin-process = "2"                       # 进程控制（exit/restart）
tauri-plugin-dialog = "2"                       # 文件对话框
tauri-plugin-deep-link = "2"                    # 深度链接
tauri-plugin-autostart = "2"                    # 自启动（LaunchAgent）

serde = { version = "1", features = ["derive"] }  # 序列化
serde_json = "1"                                # JSON 处理
log = "0.4"                                     # 日志门面

# 非移动端额外依赖
[target."cfg(not(any(target_os = \"android\", target_os = \"ios\")))".dependencies]
tauri-plugin-updater = "2"                      # 应用更新检查

# macOS 专属依赖
[target."cfg(target_os = \"macos\")".dependencies]
objc2-core-graphics = { version = "0.3.2", default-features = false, features = ["CGEvent", "CGEventTypes"] }
objc2-core-foundation = { version = "0.3.2", default-features = false, features = ["CFMachPort", "CFRunLoop"] }
```

### 6.2 依赖关系图

```
tauri ^2.10.3 (核心运行时)
  ├── tray-icon feature → 托盘/菜单 (Muda)
  ├── macOS-private-api feature → NSApp activation policy
  ├── protocol-asset feature → asset:// 资源协议
  ├── tauri-plugin-opener → URL/路径打开
  ├── tauri-plugin-process → 进程退出/重启
  ├── tauri-plugin-dialog → 文件对话框
  ├── tauri-plugin-deep-link → 深度链接
  ├── tauri-plugin-autostart → LaunchAgent 自启动
  └── tauri-plugin-updater → 应用更新

macOS 专属:
  objc2-core-graphics 0.3.2
    └── CGEventTap / CGEvent / CGEventField / CGEventMask / CGEventType
  objc2-core-foundation 0.3.2
    └── CFMachPort / CFRunLoop / kCFRunLoopDefaultMode

基础:
  serde 1 + derive → JSON 序列化
  serde_json 1 → JSON 值处理
  log 0.4 → 日志门面（配合 Tauri 内部 logger）
```

### 6.3 macOS 外部框架链接

| 框架 | 用途 | 链接方式 |
|------|------|----------|
| CoreGraphics | CGEventTap / CGGetDisplaysWithPoint | `#[link(name = "CoreGraphics", kind = "framework")]` |
| ApplicationServices | AXIsProcessTrusted() 辅助功能权限查询 | `#[link(name = "ApplicationServices", kind = "framework")]` |

### 6.4 编译配置

```toml
[profile.release]
codegen-units = 1      # 单代码生成单元（最优 LTO）
lto = true             # 链接时优化
opt-level = "s"        # 体积优化（桌宠 App 越小越好）
strip = true           # 去除调试符号
```

---

## 7. 配置文件说明

### 7.1 tauri.conf.json

| 字段 | 值 | 说明 |
|------|-----|------|
| `productName` | `WuwaTappo` | 应用显示名 |
| `identifier` | `com.kuro.wuwatappo.cn` | Bundle ID |
| `frontendDist` | `../frontend` | 前端 dist 目录（编译时嵌入） |
| `macOSPrivateApi` | `true` | 启用 macOS 私有 API |
| `main` 窗口尺寸 | 320×420 | 桌宠窗口初始尺寸 |
| `main` 窗口属性 | transparent, decorations=false, alwaysOnTop, skipTaskbar=true | 无框透明置顶不占 Dock |
| `assetProtocol` scope | `$APPLOCALDATA/**`, `$RESOURCE/**` | 允许访问的资源路径 |
| `bundle.targets` | app, dmg | 打包产物 |
| `bundle.macOS.minimumSystemVersion` | 10.15 | 最低系统版本 |
| `bundle.macOS.infoPlist` | Info.plist | 自定义 plist |

### 7.2 Info.plist

| 字段 | 值 | 说明 |
|------|-----|------|
| `CFBundleName` | WuwaTappo | 应用名 |
| `CFBundleDisplayName` | WuwaTappo | 显示名 |
| `LSUIElement` | true | **关键！** 不占 Dock，仅托盘入口（桌宠惯例） |

### 7.3 capabilities/main.json

完整镜像官方 WuwaTappo.exe 的 ACL 权限集，覆盖 9 个窗口标签（`main`, `panel`, `login`, `setting`, `toast`, `update`, `about`, `oss-notice`, `installer-shell` + 通配符），共 **173 条权限**，涵盖：

- `core:window`（完整窗口操作权限集）
- `core:tray`（托盘创建/菜单/图标/tooltip）
- `core:menu`（菜单创建/修改/popup）
- `core:webview`（WebView 创建/位置/大小/焦点）
- `core:event`（emit/emit-to/listen）
- `dialog:default` / `opener:default` / `process:default`
- `autostart` 权限（enable/disable/is-enabled）
- `updater` 权限（check/download/install）

---

## 8. 项目运行与构建

### 8.1 前置条件

| 依赖 | 版本要求 | 说明 |
|------|----------|------|
| macOS | ≥ 10.15 | Catalina 起支持 WKWebView 所有 Tauri 特性 |
| Rust stable | ≥ 1.77 | Tauri 2 最低要求 |
| Xcode Command Line Tools | ≥ 14.0 | 提供 clang / macOS SDK |
| `cargo tauri` | 2.x | tauri-cli（`cargo install tauri-cli` 或 Rustup 插件） |
| 辅助功能授权 | 必需 | CGEventTap 监听全局键鼠事件 |

| 权限 | 用途 | 说明 |
|------|------|------|
| 辅助功能（Accessibility） | CGEventTap（ListenOnly 亦可） | 鼠标/键盘事件采集的基础权限 |
| **输入监控（Input Monitoring）** | Default tap（可吞事件） | **点击不穿透桌面 + 全局键盘计数**必需；缺失时只能降级 |

### 8.2 克隆与构建

```bash
git clone https://github.com/EchoAI-01/WuwaTappo-macos.git
cd WuwaTappo-macos

# Release 构建（LTO + strip，体积优化）
cargo build --release

# 只出 .app（快）
cargo tauri build --bundles app

# 出 .app + .dmg
cargo tauri build

# 使用固定证书签名（推荐，授权可跨编译保持）
APPLE_SIGNING_IDENTITY="WuwaTappo Dev Signing" cargo tauri build --bundles app
```

产物位置：`src-tauri/target/release/bundle/`

**签名验证**：

```bash
codesign -dv src-tauri/target/release/bundle/macos/WuwaTappo.app
# adhoc（不可持久授权）: Signature=adhoc
# 固定证书（可持久）    : Signature size=1649
```

### 8.3 开发构建

```bash
cd src-tauri

# 调试模式
cargo build

# 运行开发版本
cargo tauri dev

# 产物: src-tauri/target/debug/wuwatappo-macos
```

### 8.4 运行（首次必读）

**⚠️ 测试时必须 `open WuwaTappo.app`（LaunchServices）启动——直接执行二进制会触发 WKWebView 挂起伪影（表现为桌宠无响应、计数停止），得到假 bug。**

**⚠️ 重启前务必先杀干净旧实例**（bundle id 相同会复用旧进程，导致"改了代码没效果"）：

```bash
pkill -9 -f wuwatappo; sleep 1
open src-tauri/target/release/bundle/macos/WuwaTappo.app
```

首次启动三步：

1. **辅助功能授权**：系统设置 → 隐私与安全性 → 辅助功能 勾选本应用。未授权时应用会自动打开该面板，授权后约 3 秒自动生效。
2. **输入监控授权**：系统设置 → 隐私与安全性 → **输入监控** 勾选本应用。这是 **Default tap（吞事件/全局键盘）** 的前提；未授权时 `/tmp/wuwatappo-input.log` 显示 `listen-only`，表现为点击穿透桌面、只有窗口聚焦时按键才计数。授权后重启 App 生效（或等代码内 2s 轮询自动升级）。
3. **签名与授权持久性**：`signingIdentity: "-"`（adhoc）时每次编译 cdhash 都变，macOS 视为新应用，**之前的授权全部失效**。要避免反复授权，请用固定证书签名（见 8.2）。

验证当前 tap 模式：

```bash
cat /tmp/wuwatappo-input.log
# default/swallow  → 输入监控已授权，点击不穿透
# listen-only      → 缺输入监控，点击会穿透
```

### 8.5 前端修改（踩坑 §1）

**⚠️ 前端是编译期嵌入，改 `frontend/` 必须 touch lib.rs！**

Tauri 2 的 `frontendDist` 不是运行时读盘：`build.rs` 把 dist 复制到 `target/…/out/tauri-codegen-assets`（压缩 + content-hash 文件名），经 `generate_context!` 宏**编译期嵌入二进制**。cargo 增量编译**不会**因 frontend 目录变化自动重嵌入。改前端后必须：

```bash
touch src-tauri/src/lib.rs && cargo tauri build --bundles app
```

### 8.6 WKWebView 磁盘缓存（踩坑 §2）

`~/Library/Caches/<bundle-id>` 会缓存 `tauri://localhost` 的 JS。同名文件就地修改后即使正确重编译，旧缓存仍可能生效。调试 UI 异常时先：

```bash
rm -rf ~/Library/Caches/com.kuro.wuwatappo.cn
```

> **不要**删 `~/Library/WebKit/<id>/WebsiteData`，那是 localStorage / 登录态。

### 8.7 权限授权（详细）

需要**两个独立**的 TCC 开关，缺一不可：

```
系统设置 → 隐私与安全性 → 辅助功能   → 勾选 WuwaTappo（CGEventTap 能建立）
系统设置 → 隐私与安全性 → 输入监控   → 勾选 WuwaTappo（可吞事件 + 收到键盘事件）
```

| 已授权 | tap 模式 | 表现 |
|--------|----------|------|
| 都没有 | 建不起来 | `/tmp/wuwatappo-input.log` 显示 `CGEventTapCreate failed`，桌宠点击/拖拽/计数全灭 |
| 仅辅助功能 | `listen-only` | 鼠标事件能进来、点击可响应，但**点击穿透桌面**且**收不到键盘**（只有窗口聚焦时原生 keydown 计数） |
| 辅助功能 + 输入监控 | `default/swallow` | 完整功能：点击不穿透、全局键盘计数、拖拽正常 |

输入监控缺失时，App 启动会自动打开该设置页并调用 `CGRequestListenEventAccess()` 触发系统授权弹窗。授权后**需重启 App**（TCC 对已运行进程不刷新），或等代码内每 2 秒的轮询自动升级。

> 若每次重新编译后授权都失效，说明当前是 adhoc 签名（见踩坑 §5）。改用固定证书签名后，授权可跨编译保持。

### 8.8 诊断工具

| 工具 | 位置 | 用途 |
|------|------|------|
| 命令序列日志 | `/tmp/wuwatappo-diag.log` | 所有 50 个命令入口 + hit-region payload/解析结果双写，定位前端初始化链断点 |
| 输入模式日志 | `/tmp/wuwatappo-input.log` | `default/swallow` = 输入监控已授权（吞事件）；`listen-only` = 只有辅助功能（点击穿透且收不到键盘） |
| 启动诊断 | 前 2 分钟 diag 行 | 窗口几何 / bridge_ready / regions 数量快照 |
| 应用日志 | `$APPLOCALDATA/logs/app-YYYYMMDD.log` | 前端 onerror/unhandledrejection + 10s 心跳（WKWebView 的 console 不可见） |
| 存储数据 | `$APPLOCALDATA/native-store/store.json` | 打字计数 / 点击计数持久化 |
| 标准输出 | `println!` | LaunchServices 启动时可能被丢弃，以文件为准 |

#### 快速定位命令

```bash
# 运行时状态诊断
tail -f /tmp/wuwatappo-diag.log

# 输入监听状态
cat /tmp/wuwatappo-input.log

# 应用日志
ls ~/Library/Application\ Support/com.kuro.wuwatappo.cn/native-store/logs/
cat ~/Library/Application\ Support/com.kuro.wuwatappo.cn/native-store/logs/app-*.log

# 本地存储数据
cat ~/Library/Application\ Support/com.kuro.wuwatappo.cn/native-store/store.json
```

### 8.9 自启动设置

```bash
# 启用自启动（通过 Rust 命令调用 tauri-plugin-autostart）
# LaunchAgent plist 由插件自动生成到 ~/Library/LaunchAgents/
# 前端 invoke("set_elevated_autostart", true)
```

---

## 9. 架构决策与已知限制

### 9.1 架构决策记录

| # | 决策 | 原因 | 影响范围 |
|---|------|------|----------|
| 1 | **全屏 shell 恒穿透** | 对齐官方 Windows WM_NCHITTEST 语义；桌宠的点击/拖拽/右键菜单全部由前端基于全局鼠标事件合成 | main 窗口设置 + input.rs per-region 过滤 |
| 2 | **CGEventTap HIDEventTap + HeadInsertEventTap** | BongoCat 参考；session tail 的 tap 会丢失部分 FlagsChanged（右 Shift 释放） | input.rs tap 建立逻辑 |
| 3 | **事件直转无门控** | 曾做"emit 等握手"门控，但前端初始化链的 await 可能依赖首批事件 → 形成**循环等待**（前端初始化链 await mouse-move 布局 → 门控等握手 → 握手永远不来 → 初始化链永久中断，症状随编译产物时序漂移——"时好时坏"的根源）。已回退：事件始终直转，`notify_input_bridge_ready` 仅作日志观测 | input.rs + notify_input_bridge_ready |
| 4 | **辅助窗口遮挡判定** | 辅助窗口（panel/login/toast）不穿透，点击应归属它们自己；mouse-down 需排除辅助窗口区域 | hit_test() |
| 5 | **裸数字返回打字计数** | 官方如此：`load_key_press_count` 返回裸 i64（前端 `Number(payload)` 解析），`load_cumulative_click_count` 返回对象。重建命令时勿"统一"格式，前端按官方格式解析 | load_key_press_count 返回 i64 |
| 6 | **Keep-alive eval("void 0")** | WKWebView 会对"无活动"页面执行 **idle 挂起**（JS 冻结、timer 停转）——用户点开登录窗后 main 页面即被冻结，表现为"按键统计停止、桌宠无响应"（Windows WebView2 无此行为）。每 5 秒对所有窗口 eval("void 0") 维持活跃 | setup() 后台线程 |
| 7 | **lib.rs 单文件** | 命令处理器密集时集中管理更易维护，避免多文件组织复杂度 | 整个 lib.rs |
| 8 | **Default tap 优先、ListenOnly 降级** | macOS 只有 Default（active）tap 能吞事件实现"点击不穿透"，但它额外要求「输入监控」权限；ListenOnly 只需「辅助功能」但点击必穿透且收不到键盘。因此运行时按权限探测结果动态选择，并在拿到权限后热切换 | run_worker() |
| 9 | **通道化事件发送** | tap 回调超时会让事件被系统丢弃（KeyUp 丢失 → 按键重复刷屏）。回调只做内存判断 + `try_send`，emit/写盘/主线程 API 全部移到 `input-emitter` 线程 | TapMsg + spawn_emitter |
| 10 | **几何缓存替代实时窗口查询** | Tauri 窗口 API 必须在主线程；在 tap 线程调用会导致进程静默崩溃。改为主线程维护 `SharedMainGeom` 快照，回调只读 | MainGeom + refresh_main_geom |
| 11 | **辅助窗口打开时降 main 层级** | main 是全屏 alwaysOnTop 窗口，与辅助窗口同层会盖住它们（login 可见状态为 true 但看不见）。`sync_main_layer` 按辅助窗口可见性动态切换 main 的置顶 | sync_main_layer |
| 12 | **固定证书签名** | adhoc 签名每次编译 cdhash 变化，TCC 授权失效，用户被迫反复授权。改用固定证书（`WuwaTappo Dev Signing`）后授权跨编译保持 | 构建流程 |

### 9.2 开发者必读：macOS 移植踩坑记录（改相关代码前必读）

**§1 前端是编译期嵌入，改 `frontend/` 必须 touch lib.rs**
Tauri 2 的 `frontendDist` 不是运行时读盘：build.rs 把 dist 复制到 `target/…/out/tauri-codegen-assets`（压缩 + content-hash 文件名），经 `generate_context!` 宏编译期嵌入二进制。cargo 增量编译**不会**因 frontend 目录变化自动重嵌入。改前端后必须：
```bash
touch src-tauri/src/lib.rs && cargo tauri build --bundles app
```

**§2 WKWebView 磁盘缓存**
`~/Library/Caches/<bundle-id>` 会缓存 `tauri://localhost` 的 JS。同名文件就地修改后即使正确重编译，旧缓存仍可能生效。调试 UI 异常时先 `rm -rf ~/Library/Caches/com.kuro.wuwatappo.cn`（**不要**删 `~/Library/WebKit/<id>/WebsiteData`，那是 localStorage / 登录态）。

**§3 输入桥握手：只能观测，不能门控**
曾按"tap 等前端注册完监听再转发事件"实现门控（防 IPC 轰炸竞态），结果与前端初始化链形成**循环等待**——前端初始化链的 await 依赖首批输入事件（mouse-move 布局），门控等握手，握手永远不来 → 初始化链永久中断，症状随编译产物时序漂移（"时好时坏"的根源）。已回退：事件始终直转，握手 `notify_input_bridge_ready` 仅作日志观测。

**§4 KuroSDK 的 protocol-relative URL**
官方 `isdkSrc:"//pro-cdn-sdk.kurogame.com/..."` 在 Windows（`https://tauri.localhost`）下正常，在 macOS（`tauri://localhost`）下解析成 `tauri://pro-cdn-sdk...` → SDK 加载失败 → 登录窗口是透明无边框窗口、页面空白 → **整窗不可见**，用户看到"点了登录什么都没发生"。修复：改为 `https://` 绝对地址。

**§5 adhoc 签名 → TCC 授权失效**
`signingIdentity: "-"` 每次重编译 cdhash 都变，macOS 视为新应用，辅助功能 / 输入监控授权自动失效。症状：键盘 / 全局点击 / 拖拽全灭，但面板内点击正常（不走全局监听）——**"只有面板内点击有反应"≈ tap 没建立**。诊断：`/tmp/wuwatappo-input.log` 的 `tap mode` 行。
**已配置固定证书** `WuwaTappo Dev Signing`（钥匙串中已信任）：`APPLE_SIGNING_IDENTITY="WuwaTappo Dev Signing" cargo tauri build --bundles app`，签名验证 `Signature size=1649`（而非 `adhoc`）。用固定证书后，TCC 授权可跨编译保持，**只需授权一次**。

**§6 hit_region 字段名是 left/top 不是 x/y**
前端上报矩形用 `left/top/width/height`。Rust 解析只认 `x/y` 时**不报错、静默返回 0 区域** → 桌宠点击 / 拖拽全被路由层过滤、事件穿透到桌面框选。现在已做双字段兼容。另外：`set_main_hit_region` 里**绝不能再调 `set_ignore_cursor_events`**（恒穿透只需启动时 `set_main_force_cursor_ignore(true)` 一次；重复调用一旦 Err，前端会报 hit_region_sync failed 并跳过后续同步）。

**§7 WKWebView idle 挂起**
见架构决策 §9.1-#6。表现："点登录后统计停止、桌宠无响应"。曾误诊为 IPC 竞态（见 §3）。

**§8 测试必须 `open` 启动**
nohup / 直接执行二进制启动会触发 WKWebView 挂起伪影，得到假 bug。始终用 `open xxx.app`。

**§9 计数命令返回格式不一致（官方如此）**
`load_key_press_count` 返回裸 i64（前端 `Number(payload)` 解析），`load_cumulative_click_count` 返回对象。重建命令时勿"统一"格式，前端按官方格式解析。

**§10 tap 回调阻塞 → KeyUp 被系统丢弃 → 按键疯狂重复**
CGEventTap 回调超过系统阈值（约 0.25s）时，**该事件会被直接丢弃**。曾经在回调里直接 `emit_to` / `dlog` 写盘 / `app.state()` 查询，一旦 KeyUp 被丢，系统认为按键一直按着 → 触发键盘重复。实测症状：**按 Cmd+Q 退出时字符刷屏，必现**。
修复：回调改为「纯内存判断 + `try_send`」，所有 emit / 写盘 / 主线程 API 移到独立 `input-emitter` 线程；`hit_test` 用 `try_lock` 不再可能阻塞在几何锁上。

**§11 计数器"用一会儿就彻底失效"**
前端 `index-DmC0wUEw.js` 的 `_c()/kc()` 用 `Pr` Set 做按键去重：keydown 时 `Pr.add(key)`，keyup 时 `Pr.delete(key)`。**keyup 一旦丢失（tap 被系统禁用期间），该键会永久卡在 Set 里**，后续按同一键会命中 `Pr.has(e)` 直接 return，表现为计数彻底失效。
修复：`TapContext.pressed_keys`（`AtomicU64` 位图）记录按下状态；tap 被禁用重启时 `swap(0)` 取出所有仍按下的键，逐个补发 `key-release`，清理前端残留。

**§12 tap 线程禁止调用窗口 API**
Tauri/tao 的 `get_webview_window` / `outer_position` / `is_visible` 必须在主线程。曾在 tap 回调里直接调用做命中测试，导致进程**不定时静默崩溃**（用户侧表现为"程序莫名消失"）。
修复：主线程维护 `SharedMainGeom` 几何快照（`refresh_main_geom`，100ms 节流），tap 回调只读快照。

**§13 全屏 main 窗口会盖住辅助窗口**
`main` 是 `alwaysOnTop: true` 的全屏 floating 窗口，与 login/panel 同层。macOS 同层窗口按最后 `orderFront` 排序，main 一旦被激活就盖住辅助窗口——实测出现 `login: visible=true` 但用户完全看不见。
修复：`sync_main_layer` —— 只要有可见辅助窗口就把 main 降为非置顶，全部关闭后恢复。注意**不要**用 `set_background_color` 给辅助窗口铺底色（会在圆角卡片外露出方形色块）。

**§14 Cmd+Q 退出后旧实例复用**
bundle id 相同时 `open xxx.app` 会复用已运行实例，导致"改了代码没效果"。每次测试前必须 `pkill -9 -f wuwatappo` 再 `open`。

### 9.3 Win32 → macOS 降级映射

| Windows 专属 API | macOS 等价 | 说明 |
|-----------------|------------|------|
| `WM_NCHITTEST` per-region 命中 | `hit_test()` 逻辑判定 + Default tap 吞事件 | Win32 原生消息路由；macOS 靠"吞事件"或"窗口动态穿透"两条路径 |
| `SetWindowRgn` 区域穿透 | `set_ignore_cursor_events(true/false)` | macOS WebKit 原生穿透支持 |
| `SetCapture` 鼠标捕获 | 全局 CGEventTap + 事件合成 | macOS 无等价捕获，靠全局事件桥兜底 |
| `HMENU` 坐标弹出 | `Muda ContextMenu::popup()` | 原生 NSMenu 跟随鼠标位置弹出 |
| `RegisterHotKey` 全局热键 | 当前未实现（预留） | 可通过 CGEventTap + 前端逻辑实现 |
| `SetWindowsHookEx` 键鼠 hook | `CGEventTap` | macOS 官方全局事件监听 API |
| 任务栏 `SHGetRect` | 返回零 insets | macOS 桌宠不与 Dock 重叠，无需避让 |
| 全屏 shell 模式 | 故意返回 Err → 前端走 fallback | Windows 专属窗口层级技巧 |

### 9.4 已知限制

| 限制 | 说明 | 优先级 |
|------|------|--------|
| **无 darwin 更新通道** | 官方 updater 服务器仅有 windows-x86_64 条目，darwin 目标缺失；跟随官方版本需手动更新前端 dist 并重编译 | 低（社区移植，不影响核心功能） |
| **联机房间 / 开箱等玩法** | 依赖官方服务端可用性，与官方 exe 端同源同命运 | 低（非本项目控制范围） |
| **辅助功能 + 输入监控需授权** | 两个独立 TCC 开关：辅助功能决定能否建 tap，输入监控决定能否 **吞事件 / 监听键盘**。用固定证书签名后授权可跨编译保持（详见踩坑 §5） | 中 |
| **全屏 shell 未实现** | `set_main_window_fullscreen_shell` 故意返回 Err，前端用 window API fallback（功能等价但实现路径不同） | 中 |
| **小键盘 / JIS 键盘键码映射** | 经 BongoCat 表校准，未在全键盘布局实测 | 低 |
| **dmg 未公证** | adhoc 签名，他人下载首次打开需右键 → 打开 | 低 |
| **多显示器 scale 估算** | `display_scale_at()` 物理像素/逻辑点计算，精度略低于 NSScreen.backingScaleFactor | 低 |

### 9.5 源码中的 no-op 命令

以下命令在 macOS 上为空实现（保留是为了前端命令协议完整性）：

| 命令 | 原由 |
|------|------|
| `set_main_swallow_mouse` | Win32 穿透切换瞬间吞掉鼠标消息防闪烁；macOS 无此问题 |
| `set_overlay_cursor` | Win32 宠物 hover 换手型光标；macOS 系统自管 |
| `finish_main_window_monitor_switch` | Windows 恢复输入劫持；macOS 无劫持概念 |
| `get_main_desktop_safe_area` | Windows 任务栏避让；macOS 返回零 |
| `get_main_taskbar_insets` | 同上 |

---

## 10. 事件协议参考

### 10.1 Tauri Events（Rust emit → 前端 listen）

| Event Name | Payload | 触发场景 |
|------------|---------|----------|
| `key-press` | `"W"`, `"F5"`, `"Shift"`, `"ArrowUp"` | 用户按键（CGEventTap → 通道 → emitter 线程）**需输入监控权限** |
| `key-release` | 同上 | 按键释放（tap 重启时会补发 stale release） |
| `mouse-move` | `{x: 1440.0, y: 900.0}` (物理像素) | 鼠标移动（节流到 60fps） |
| `mouse-button` | `{button: "left"|"right", action: "press"|"release"}` | 鼠标按下/释放（不因命中与否而丢弃，前端自行判定） |
| `dock-open-panel` | `"bag"`, `"atlas"`, `"exchange"`, `"room"`, `"settings"` | 托盘左键点击 / 打开菜单项 |
| `tray-toggle-pet-visibility` | `()` | 菜单项：隐藏/显示桌宠 |
| `tray-toggle-window-passthrough` | `()` | 菜单项：点击穿透切换 |
| `tray-set-pet-scale` | `125.0` (f64) | 菜单项：设置桌宠缩放 |
| `tray-set-window-opacity` | `80.0` (f64) | 菜单项：设置透明度 |
| `tray-restart` | `()` | 菜单项：重启 |
| `tray-quit` | `()` | 菜单项：退出 |
| `maintenance-notice-open` | `Option<Value>` | 维护公告 |
| `panel-open-tab` | `"bag"`, ... | 指定 panel 聚焦 tab |
| `update-detail` | `Option<Value>` | 更新详情 |

### 10.2 前端 Invoke → Rust Command 分组

```
┌──────────────────────────────────────────────────────┐
│                    Vue SPA 前端                       │
│                                                      │
│  invoke("command_name", { arg1: val1, arg2: val2 })  │
│     │                                                │
│     │  Tauri IPC (JSON over WebSocket)               │
│     ▼                                                │
│  Rust 命令处理器 (lib.rs 中 #[tauri::command])      │
└──────────────────────────────────────────────────────┘

命令分组索引:
  [更新/遥测] check_app_update, get_track_os_info, is_force_update_locked,
             open_maintenance_notice, show_toast_overlay, sync_native_menu,
             write_app_log
  [主窗口]   set_main_hit_region, set_main_force_cursor_ignore,
             set_main_swallow_mouse, set_main_skip_taskbar,
             set_main_window_content_bounds, set_overlay_cursor,
             sync_main_window_to_current_monitor,
             popup_main_dock_context_menu, get_boot_diagnostics,
             is_devtools_hotkeys_build, is_internal_testing_build
  [显示器]   apply_main_window_position, set_main_window_fullscreen_shell,
             set_main_window_monitor_bounds, get_main_desktop_safe_area,
             get_main_taskbar_insets, finish_main_window_monitor_switch,
             sync_aux_windows_to_main_monitor
  [维护]     get_app_update_detail, get_maintenance_notice_payload,
             has_maintenance_notice, hide_main_after_maintenance_notice_ready
  [Panel]    open_panel_window, close_panel_window,
             raise_panel_window_above_pets, set_panel_window_collapsed
  [Login]    open_login_window, close_login_window,
             resize_login_window_to_content
  [Update]   open_update_window, close_update_window
  [About]    open_about_window, close_about_window
  [OSS]      open_oss_notice_window, close_oss_notice_window
  [存储]     load_key_press_count, save_key_press_count,
             load_cumulative_click_count, save_cumulative_click_count
  [退出]     notify_input_bridge_ready, exit_app
```

---

## 附录 A：版本历史

| 版本 | 日期 | 内容 |
|------|------|------|
| v0.0.1-Beta | 2026-10-28 | 首个开源版本：壳重建完成，修复 hit_region 字段解析（left/top）、KuroSDK https 加载、计数持久化、WKWebView keep-alive、面板 1240×800 适配 |

> 版本号说明：壳（本仓库）独立版本号 v0.x；`Cargo.toml` / `tauri.conf.json` 中的 1.0.1 沿袭官方前端资产版本，便于对应官方功能集。

## 附录 B：桌宠缩放与透明度档位

```rust
const PET_SCALE_STEPS: [f64; 7] = [50.0, 75.0, 100.0, 125.0, 150.0, 175.0, 200.0];
// 前端常量: PET_SCALE_MIN=50 / MAX=200 / DEFAULT=100

const OPACITY_STEPS: [f64; 5] = [20.0, 40.0, 60.0, 80.0, 100.0];
```

## 附录 C：诊断文件快速定位

```bash
# 运行时状态诊断
tail -f /tmp/wuwatappo-diag.log

# 输入监听状态
cat /tmp/wuwatappo-input.log

# 应用日志（前端 onerror/unhandledrejection）
ls ~/Library/Application\ Support/com.kuro.wuwatappo.cn/native-store/logs/
cat ~/Library/Application\ Support/com.kuro.wuwatappo.cn/native-store/logs/app-*.log

# 本地存储数据
cat ~/Library/Application\ Support/com.kuro.wuwatappo.cn/native-store/store.json
```

## 附录 D：参考项目

- [BongoCat](https://github.com/ayangweb/BongoCat) (Apache-2.0) — CGEventTap 挂法（HIDEventTap + HeadInsertEventTap）与键码→HID usage 映射表参考
- [Tauri v2 文档](https://v2.tauri.app/) — 框架核心 API
- [README.md](file:///Users/chen/AI/桌宠/WuwaTappo-macos/README.md) — 项目说明、功能状态表、构建与运行完整说明、踩坑记录完整版
- 库洛游戏（Kuro Games）— WuwaTappo 本体与云端服务

## 附录 E：致谢与免责声明

- **致谢**：BongoCat（Apache-2.0）提供 macOS CGEventTap 参考；库洛游戏提供 WuwaTappo 本体。
- **免责声明**：本项目为社区移植，与库洛游戏 / Kuro Games 无关。鸣潮（Wuthering Waves）及相关资产版权归其所有者所有。本项目不破解、不代理、不重实现任何官方服务，仅复用官方公开客户端资产并直连官方服务端，仅供个人学习研究使用。

---

*文档生成日期：2026-10-28 | 基于 lib.rs 1334 行 + input.rs 539 行源码分析*  
*同步自 README.md v0.0.1-Beta（2026-10-28 更新）*
