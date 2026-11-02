//! 房间 socket.io 本地中继代理（macOS 移植专属修复）。
//!
//! 根因（11-02 curl 实锤）：房间长连接为 socket.io（前端 index-DmC0wUEw.js
//! `Ig()` 默认 `wss://api-mc-wuwatappo.kurogames-global.com/socket.io`），
//! 服务端握手校验 Origin 白名单：
//!   - `http://tauri.localhost`（官方 Windows WebView2 origin）→ 101 放行
//!   - `tauri://localhost`（macOS WKWebView origin）→ **403 拒绝**
//!   - 无 Origin → 放行
//! WKWebView 发起 WebSocket 时自动携带页面 origin，前端 JS 无法修改该头 →
//! socket 永远连不上 → 创建/加入房间按钮置灰（前置检查 socket_disconnected）。
//!
//! 方案：本地监听 `127.0.0.1:17321`（前端 `Ig()` 已补丁指向代理），accept
//! 前端连接后以 `Origin: http://tauri.localhost` 外连官方 wss，双向透传帧。
//! 页面 origin 为 tauri://（非 secure context），连 ws:// 不受混合内容限制；
//! 代理只绑回环地址、目标 host 硬编码，无额外攻击面。

use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{accept_hdr_async, connect_async};

/// 代理监听端口（前端补丁 `Ig()` 硬编码同值，两处必须一致）
pub const PROXY_PORT: u16 = 17321;
/// 官方房间服务 host（`Ig()` 默认值，wsUrl 从未被外部传入）
const TARGET_HOST: &str = "api-mc-wuwatappo.kurogames-global.com";
/// 服务端白名单内的 Origin（官方 Windows WebView2 页面 origin）
const SPOOF_ORIGIN: &str = "http://tauri.localhost";

/// 启动代理（app setup 时调用；bind 失败仅记日志，不影响其余功能）
pub fn spawn() {
    tauri::async_runtime::spawn(async move {
        let addr = format!("127.0.0.1:{PROXY_PORT}");
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                crate::dlog(&format!("[ws-proxy] listening on {addr}"));
                loop {
                    match listener.accept().await {
                        Ok((stream, peer)) => {
                            tauri::async_runtime::spawn(handle_conn(stream, peer));
                        }
                        Err(e) => crate::dlog(&format!("[ws-proxy] accept failed: {e}")),
                    }
                }
            }
            Err(e) => crate::dlog(&format!(
                "[ws-proxy] bind {addr} failed: {e}（房间功能将不可用）"
            )),
        }
    });
}

/// 单条连接：握手（捕获 path+query）→ 外连官方（重写 Origin）→ 双向透传
async fn handle_conn(client: TcpStream, peer: std::net::SocketAddr) {
    // accept_hdr_async 的回调是唯一能拿到前端请求 URI 的钩子（socket.io 的
    // path 与 query——含 infoToken 凭据——必须原样带给官方服务端）
    let uri: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let uri_cb = uri.clone();
    let callback = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        if let Some(pq) = req.uri().path_and_query() {
            *uri_cb.lock().unwrap() = Some(pq.as_str().to_string());
        }
        Ok(resp)
    };
    let ws_client = match accept_hdr_async(client, callback).await {
        Ok(ws) => ws,
        Err(e) => {
            crate::dlog(&format!("[ws-proxy] handshake from {peer} failed: {e}"));
            return;
        }
    };
    let path_query = uri
        .lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| "/socket.io/".into());
    let target = format!("wss://{TARGET_HOST}{path_query}");

    let mut req = match target.clone().into_client_request() {
        Ok(r) => r,
        Err(e) => {
            crate::dlog(&format!("[ws-proxy] bad target: {e}"));
            return;
        }
    };
    // 关键一步：Origin 重写为服务端白名单值
    req.headers_mut()
        .insert("Origin", HeaderValue::from_static(SPOOF_ORIGIN));
    let (ws_up, resp) = match connect_async(req).await {
        Ok(pair) => pair,
        Err(e) => {
            crate::dlog(&format!("[ws-proxy] upstream connect failed: {e} target={target}"));
            return;
        }
    };
    crate::dlog(&format!(
        "[ws-proxy] relay established: {peer} -> {TARGET_HOST}{} (upstream {})",
        path_query,
        resp.status()
    ));

    // 双向透传（Text/Binary/Ping/Pong/Close 全转发）；任一方向结束即整体收尾
    let (mut c_sink, mut c_stream) = ws_client.split();
    let (mut u_sink, mut u_stream) = ws_up.split();
    let up = async move {
        while let Some(msg) = c_stream.next().await {
            match msg {
                Ok(m) => {
                    if u_sink.send(m).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };
    let down = async move {
        while let Some(msg) = u_stream.next().await {
            match msg {
                Ok(m) => {
                    if c_sink.send(m).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    };
    futures_util::future::select(Box::pin(up), Box::pin(down)).await;
    crate::dlog(&format!("[ws-proxy] relay closed: {peer}"));
}
