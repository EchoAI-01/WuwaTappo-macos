# WuwaTappo-macos

鸣潮桌宠 WuwaTappo 的 macOS 移植版（社区移植，非官方）。

将 Windows 版 `WuwaTappo.exe`（Tauri 2 构建）的前端 dist 完整复用，并为 macOS 重建原生壳：全局键鼠监听、恒穿透全屏 shell、托盘菜单、面板窗口与本地存储命令。

## 功能

- 桌宠显示 / 拖拽 / 点击交互（全局 CGEventTap 事件桥 + 前端合成）
- 打字与点击计数（本地 `native-store/store.json` 持久化）
- 背包 / 图鉴 / 兑换 / 房间 / 设置面板（1040×720，尺寸可调不可缩）
- 原生托盘与右键菜单（缩放 / 透明度 / 穿透 / 重启 / 退出）
- 初次使用引导、关于、OSS 许可等辅助窗口

## 架构要点

- **全屏 shell 恒穿透**：main 窗口 WebView 常驻 `ignoresMouseEvents`，桌宠的点击 / 拖拽 / 右键菜单全部由前端基于 `mouse-move` / `mouse-button` 全局事件合成（对齐官方 Windows 的 WM_NCHITTEST 语义，macOS 等价实现为 per-region 命中过滤）。
- **输入桥就绪握手**：CGEventTap 在前端完成事件监听注册（`notify_input_bridge_ready`）前不向 WebView 转发事件——避免高频事件在 WebView 初始化期间轰炸 IPC 通道，与前端初始化链产生竞态导致监听注册失败（计数与桌宠交互全部失效）。
- **辅助功能权限**：全局键鼠监听需要「辅助功能」授权；未授权时自动打开系统设置面板提示。
- **WKWebView 缓存坑**：前端 dist 为编译期嵌入，修改 `frontend/` 后必须重新编译 Rust 才会生效。

## 构建

```bash
cd src-tauri
cargo build --release        # 调试构建
cargo tauri build            # 打包 .app 与 .dmg
```

产物位于 `src-tauri/target/release/bundle/`。

## 已知限制

- 更新检查：官方服务器无 darwin 更新通道，恒显示「无更新」。
- 联机房间等需登录功能依赖官方服务端，可用性以服务端为准。

## 致谢

- [BongoCat](https://github.com/ayangweb/BongoCat)（Apache-2.0）：macOS CGEventTap 挂法与键码映射参考。

## 免责声明

本项目为社区移植，与库洛游戏 / Kuro Games 无关。鸣潮（Wuthering Waves）及相关资产版权归其所有者所有，仅供个人学习研究使用。
