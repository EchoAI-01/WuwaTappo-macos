# WuwaTappo-macos

![platform](https://img.shields.io/badge/platform-macOS%2010.15%2B-blue) ![tauri](https://img.shields.io/badge/Tauri-2-FFC131) ![version](https://img.shields.io/badge/version-v0.0.1--Beta-orange) ![status](https://img.shields.io/badge/status-beta%20测试中-yellow)

鸣潮桌宠 **WuwaTappo** 的 macOS 移植版（社区移植，非官方）。

官方只发布了 Windows 版（exe）。本项目通过逆向官方 Windows 安装包，**完整复用官方前端与云端协议，为 macOS 重建原生外壳**，让 Mac 用户也能使用鸣潮桌宠，并与 Windows exe 端玩家正常联机互动。

> 仓库：https://github.com/EchoAI-01/WuwaTappo-macos

---

## 一、开发目标

| 项 | 内容 |
|---|---|
| 原始需求 | 官方 WuwaTappo 仅支持 Windows，macOS 无官方版本 |
| 目标 1 | 逆向 `WuwaTappo-cn_1.0.1_x64-setup.exe`，产出 macOS 可用的 `.app` / `.dmg` |
| 目标 2 | **与 exe 端联机互通**：直接对接官方服务端 API 与官方 KuroSDK 登录（非私服、非协议破解），Windows / macOS 玩家可进同一房间互动 |
| 目标 3 | 保留官方端全部功能：桌宠展示 / 换装 / 表情 / 开箱 / 合成 / 兑换码 / 房间 / 计数统计 |

### 移植策略（为什么是"复用前端 + 重建壳"）

曾对比三条路线：A. 虚拟机/兼容层跑 exe；B. **复用官方前端 dist，为 macOS 重建 Rust 原生壳**；C. 参照 BongoCat 用原生技术重写 UI。

最终选定 **方案 B**，理由：

- 官方前端（Vue 3 + Vite）承载了几乎全部业务逻辑：皮肤系统（260 皮肤 × 分层 PNG）、开箱 / 合成 / 房间玩法、KrSDK 登录、多语言。重写不可维护，且无法跟随官方更新。
- 官方后端是 Tauri 2（Rust），其自定义命令签名在未 strip 的二进制里可直接读取，按官方签名 1:1 重建即可与前端无缝对接，重建面远小于预估。
- 云端协议（REST + JSON）与平台无关，macOS 直连即可与 exe 端互通。

### 逆向工作流（怎么做到的）

1. `7zz` 双层解开 Tauri 安装壳 + 内嵌 NSIS 包，取出 `WuwaTappo.exe`（77.9 MB）与前端 dist；
2. 分析未 strip 的 Rust 符号，还原 50 个自定义 invoke 命令与窗口模型；
3. 实测官方协议链路（配置发现 → 资源 CDN → 登录 → 玩法 API），确认 macOS 可直连；
4. 为 macOS 重建壳（本仓库 `src-tauri/`），对齐官方关键架构决策（全屏 shell 恒穿透 + 输入事件桥）；
5. 对官方前端 dist 做**最小限度补丁**（见下文"前端补丁清单"），其余字节不动。

---

## 二、功能状态（v0.0.1-Beta）

| 功能 | 状态 | 说明 |
|---|---|---|
| 桌宠显示 / 缩放 / 透明度 / 置顶置底 | ✅ | 皮肤资源直连官方 CDN |
| 换装 / 配饰 / 表情 | ✅ | 官方 appearance 资产（260 皮肤 / 135 配饰 / 504 表情） |
| 桌宠拖拽 / 点击 / 右键菜单 | ✅ 已修复待实测 | 全屏 shell 恒穿透 + CGEventTap 事件桥（详见架构） |
| 打字 / 点击计数 | ✅ 已修复待实测 | 持久化于 `native-store/store.json`，重启不丢 |
| 背包 / 图鉴 / 兑换 / 设置面板 | ✅ | 1240×800 无边框透明窗口 |
| 登录（KuroSDK 国服） | ✅ 已修复待实测 | KrSDK 动态加载（详见踩坑 §4） |
| 联机房间（与 exe 端同房间） | ⚠️ 待实测 | 走官方 API，登录后可用 |
| 开箱 / 合成 / 收藏 / 兑换码 | ⚠️ 待实测 | 官方玩法 API |
| 原生托盘菜单 | ✅ | 缩放 / 透明度 / 穿透 / 显隐 / 重启 / 退出，多语言同步 |
| 自动更新 | ❌ | 官方 updater 无 darwin 通道，恒显示"无更新" |

> "已修复待实测"：v0.0.1-Beta 已修复对应根因（见版本历史），等待更多真机验证。

---

## 三、架构

### 3.1 总览

```
┌────────────────────────────────────────────────────────────┐
│  官方云服务（Kuro）                                          │
│  配置发现 / 资源CDN / KrSDK登录 / 玩法API / 房间             │
└──────────────────────▲─────────────────────────────────────┘
                       │ REST + JSON（平台无关，与 exe 端同一协议）
┌──────────────────────┴─────────────────────────────────────┐
│  前端 dist（官方 Vue3+Vite 产物，编译期嵌入二进制）            │
│  index.html + hash 路由：#/桌宠  #/panel  #/login  #/about…  │
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

### 3.2 窗口模型（对齐官方）

- **main**：2560×1440 全屏透明 shell，覆盖整个屏幕、`ignoresMouseEvents` **恒穿透**、多工作区可见。桌宠本体绘制在右下角 CSS 区域。`tauri.conf.json` 中的 320×420 仅是初始值，启动后由前端 fallback 链路调整为全屏 shell（官方同款时序）。
- **panel / login / about / update / oss-notice**：独立无边框透明窗口，与 main 共用同一个 `index.html`，靠 hash 路由区分页面。不穿透，正常接收点击。
- 所有窗口由 Rust 侧 `create_aux_window` 统一创建。

### 3.3 交互管线（本移植的核心）

官方 Windows 版用 Win32 per-region 命中测试（`WM_NCHITTEST`）实现"桌宠可点、空白处穿透"。macOS 等价实现为**恒穿透 + 事件桥 + 前端合成**：

```
CGEventTap（全局，ListenOnly）
  │ mouse-move（物理像素，约 60fps 节流）
  │ mouse-button（press/release × left/right，press 经 per-region 命中过滤）
  │ key-press / key-release（KeyboardEvent.key 风格字符串）
  ▼
emit → main 窗口前端
  │ elementFromPoint + click()      → 合成按钮点击（引导按钮/宝箱/设置）
  │ beginDockPointerAt 等手势函数    → 合成拖拽
  │ fc()                            → 右键 → invoke popup_main_dock_context_menu
  ▼
前端把桌宠可交互区域经 set_main_hit_region 上报 Rust
  （inputRegions + regions，字段 left/top/width/height）
  → Rust 侧 route_mouse_press 据此过滤：命中桌宠区域才转发 press，
    点在辅助窗口上、或点在桌宠区域外 → 不转发（事件自然落到下层应用）
```

**坐标换算**：CGEvent location 是全局逻辑点，emit 前按鼠标所在显示器 backing scale 乘算成物理像素；前端 `(全局物理坐标 − window.innerPosition) / scaleFactor` 还原为 CSS 坐标。

### 3.4 keep-alive 线程（macOS 特有）

WKWebView 会对"无活动"页面执行 **idle 挂起**（JS 冻结、timer 停转）——用户点开登录窗后 main 页面即被冻结，表现为"按键统计停止、桌宠无响应"（Windows WebView2 无此行为）。Rust 侧后台线程每 5 秒对所有窗口 `eval("void 0")` 维持活跃。

### 3.5 前端补丁清单（对官方 dist 的全部修改）

官方 dist 除以下补丁外字节不动：

| 文件 | 补丁 | 原因 |
|---|---|---|
| `assets/index-D8GJxFG7.js` | 应用显示文案 | 应用名适配 |
| | `isdkSrc` / `isdkCss` 由 `//pro-cdn-sdk.kurogame.com/...` 改为 `https://...` | protocol-relative URL 在 `tauri://localhost` 下解析失败，KrSDK 加载不了，登录窗口空白（Windows 的 `https://tauri.localhost` 不受影响） |
| `assets/index-DmC0wUEw.js` | `key-press` 监听注册后追加 `notify_input_bridge_ready()` 注入 | 输入桥握手观测点（见踩坑 §3） |
| `assets/index-DenkafYz.js` | 文案适配 | 同应用名适配 |

---

## 四、项目结构

```
WuwaTappo-macos/
├── frontend/                 # 官方前端 dist（逆向提取 + 最小补丁，编译期嵌入）
│   ├── index.html            # hash 路由入口
│   └── assets/               # Vue3+Vite 产物：JS chunk / 字体 / 皮肤图（60MB，127 文件）
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs            # 壳主体：50 个命令 / 窗口管理 / 托盘 / keep-alive / 计数持久化
│   │   ├── input.rs          # CGEventTap 全局键鼠监听 / per-region 路由 / 键码映射
│   │   └── main.rs           # 入口
│   ├── tauri.conf.json       # macOSPrivateApi=true（穿透所需）/ adhoc 签名
│   ├── Info.plist
│   ├── capabilities/         # Tauri 权限声明
│   └── icons/
├── README.md
└── .gitignore
```

> 逆向分析材料（exe 原件、解包产物、完整分析报告）**不在本仓库内**，仅保留分析结论于本文档。

---

## 五、构建与运行

### 前置要求

- macOS 10.15+
- Rust stable + Xcode Command Line Tools
- `cargo tauri`（tauri-cli 2.x）

### 命令

```bash
git clone https://github.com/EchoAI-01/WuwaTappo-macos.git
cd WuwaTappo-macos

cargo tauri build --bundles app   # 只出 .app（快）
cargo tauri build                 # 出 .app + .dmg
```

产物位于 `src-tauri/target/release/bundle/`。

### 运行（首次必读）

1. **辅助功能授权**：全局键鼠监听（计数 / 拖拽 / 点击）需要 系统设置 → 隐私与安全性 → 辅助功能 中勾选本应用。未授权时应用会自动弹出该面板，授权后约 3 秒自动生效，无需重启。
2. **adhoc 签名提示**：本仓库未配置开发者证书（`signingIdentity: "-"`）。自己编译的版本未公证，首次打开需右键 → 打开；且**每次重新编译都被 macOS 视为新应用，辅助功能授权会失效需重新勾选**（详见踩坑 §5）。从 Release 下载的版本不受此影响（签名固定）。
3. 测试时请用 `open WuwaTappo.app`（LaunchServices）启动——直接执行二进制会触发 WKWebView 挂起伪影（详见踩坑 §8）。

---

## 六、开发者必读：macOS 移植踩坑记录

以下是本项目踩过并已修复的坑，**改相关代码前必读**：

**§1 前端是编译期嵌入，改 `frontend/` 必须 touch lib.rs**
Tauri 2 的 `frontendDist` 不是运行时读盘：build.rs 把 dist 复制到 `target/…/out/tauri-codegen-assets`（压缩 + content-hash 文件名），经 `generate_context!` 宏编译期嵌入二进制。cargo 增量编译**不会**因 frontend 目录变化自动重嵌入。改前端后必须：

```bash
touch src-tauri/src/lib.rs && cargo tauri build --bundles app
```

**§2 WKWebView 磁盘缓存**
`~/Library/Caches/<bundle-id>` 会缓存 `tauri://localhost` 的 JS。同名文件就地修改后即使正确重编译，旧缓存仍可能生效。调试 UI 异常时先 `rm -rf ~/Library/Caches/com.kuro.wuwatappo.cn`（**不要**删 `~/Library/WebKit/<id>/WebsiteData`，那是 localStorage / 登录态）。

**§3 输入桥握手：只能观测，不能门控**
曾按"tap 等前端注册完监听再转发事件"实现门控（防 IPC 轰炸竞态），结果与前端初始化链形成**循环等待**——前端初始化链的 await 依赖首批输入事件（mouse-move 布局），门控等握手，握手永远不来 → 初始化链永久中断，症状随编译产物时序漂移（"时好时坏"的根源）。已回退：事件始终直转，握手 `notify_input_bridge_ready` 仅作日志观测。

**§4 KrSDK 的 protocol-relative URL**
官方 `isdkSrc:"//pro-cdn-sdk.kurogame.com/..."` 在 Windows（`https://tauri.localhost`）下正常，在 macOS（`tauri://localhost`）下解析成 `tauri://pro-cdn-sdk...` → SDK 加载失败 → 登录窗口是透明无边框窗口、页面空白 → **整窗不可见**，用户看到"点了登录什么都没发生"。修复：改为 `https://` 绝对地址。

**§5 adhoc 签名 → TCC 授权失效**
`signingIdentity: "-"` 每次重编译 cdhash 都变，macOS 视为新应用，辅助功能授权自动失效。症状：键盘 / 全局点击 / 拖拽全灭，但面板内点击正常（不走全局监听）——**"只有面板内点击有反应"≈ tap 没建立**。诊断：看 `/tmp/wuwatappo-input.log` 是否有 `NOT granted`。根治方案（未做）：固定 self-signed codesigning 证书。

**§6 hit_region 字段名是 left/top 不是 x/y**
前端上报矩形用 `left/top/width/height`。Rust 解析只认 `x/y` 时**不报错、静默返回 0 区域** → 桌宠点击 / 拖拽全被路由层过滤、事件穿透到桌面框选。现在已做双字段兼容。另外：`set_main_hit_region` 里**绝不能再调 `set_ignore_cursor_events`**（恒穿透只需启动时 `set_main_force_cursor_ignore(true)` 一次；重复调用一旦 Err，前端会报 hit_region_sync failed 并跳过后续同步）。

**§7 WKWebView idle 挂起**
见架构 §3.4。表现："点登录后统计停止、桌宠无响应"。曾误诊为 IPC 竞态（见 §3）。

**§8 测试必须 `open` 启动**
nohup / 直接执行二进制启动会触发 WKWebView 挂起伪影，得到假 bug。始终用 `open xxx.app`。

**§9 计数命令返回格式不一致（官方如此）**
`load_key_press_count` 返回裸 i64（前端 `Number(payload)` 解析），`load_cumulative_click_count` 返回对象。重建命令时勿"统一"格式，前端按官方格式解析。

### 诊断工具

| 工具 | 位置 | 用途 |
|---|---|---|
| 命令序列日志 | `/tmp/wuwatappo-diag.log` | 所有 50 个命令入口 + hit-region payload/解析结果双写，定位前端初始化链断点 |
| 输入权限日志 | `/tmp/wuwatappo-input.log` | 出现 `NOT granted` = 辅助功能未授权（tap 未建立） |
| 启动诊断 | 前 2 分钟 diag 行 | 窗口几何 / bridge_ready / regions 数量快照 |

---

## 七、已知限制

- 自动更新：官方 updater 无 darwin 通道，恒"无更新"；跟随官方版本需手动更新前端 dist 并重编译。
- 联机房间 / 开箱等玩法依赖官方服务端可用性，与官方 exe 端同源同命运。
- 小键盘 / JIS 键盘键码映射经 BongoCat 表校准，未在全键盘布局实测。
- dmg 未公证（adhoc 签名），他人下载首次打开需右键 → 打开。

---

## 八、版本历史

| 版本 | 日期 | 内容 |
|---|---|---|
| v0.0.1-Beta | 2026-10-28 | 首个开源版本：壳重建完成，修复 hit_region 字段解析（left/top）、KrSDK https 加载、计数持久化、WKWebView keep-alive、面板 1240×800 适配 |

> 版本号说明：壳（本仓库）独立版本号 v0.x；`Cargo.toml` / `tauri.conf.json` 中的 1.0.1 沿袭官方前端资产版本，便于对应官方功能集。

---

## 九、致谢

- [BongoCat](https://github.com/ayangweb/BongoCat)（Apache-2.0）：macOS CGEventTap 挂法（HIDEventTap + HeadInsertEventTap，否则丢 FlagsChanged）与键码→HID usage 映射表参考。
- 库洛游戏（Kuro Games）：WuwaTappo 本体与云端服务。

## 十、免责声明

本项目为社区移植，与库洛游戏 / Kuro Games 无关。鸣潮（Wuthering Waves）及相关资产版权归其所有者所有。本项目不破解、不代理、不重实现任何官方服务，仅复用官方公开客户端资产并直连官方服务端，仅供个人学习研究使用；若官方提出异议，本项目将立即下架处理。
