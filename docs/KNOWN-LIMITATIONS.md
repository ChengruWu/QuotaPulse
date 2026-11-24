# 已知限制

- **非公开只读接口风险**：Codex（ChatGPT）与 Antigravity（Google Cloud Code）额度接口均非稳定公开 API，字段、域名或认证方式可能随上游版本变动；出现异常时应用会展示 `stale` / 不可用状态，绝不伪造估算数值。
- **Antigravity 数据源优先级**：后台轮询前先静默执行 `AGY_CLI_CDE_AUTH_ACTION=check` 预检登录态（未登录时直接返回 `signed_out`，不会在后台拉起浏览器登录页）；登录有效时静默调用本机官方 `agy.exe --log-file %TEMP%\quotapulse-agy-poll.log --output-format json --print-timeout 15s -p "/usage"`（零模型消耗、`AGY_CLI_DISABLE_AUTO_UPDATE=true`、`AGY_CLI_NONINTERACTIVE_HEADLESS=1`）。未登录时可在 QuotaPulse 内直接点击 **登录 Gemini**（调用官方 `agy.exe` 的 `AGY_CLI_CDE_AUTH_ACTION=login` 完成浏览器本地回调授权，无需启动 Antigravity 应用或粘贴验证码）；仅当未检测到 `agy.exe` 或其失败时，回退到只读读取 `state.vscdb` / `~/.gemini` 本地凭据（QuotaPulse 不会硬编码第三方 OAuth Client Secret，也不会回写 `state.vscdb`）。
- **差异化轮询频率**：为避免对 Google 接口高频请求，Antigravity 后台轮询默认每 60 秒一次（Codex 维持 10 秒一次）；点击托盘菜单或面板右上角的“刷新”可立即并行刷新全部服务。
- **未进行 Windows 代码签名**：当前默认发布包为 unsigned 构建（带 Tauri 自动更新签名，但无 Windows Authenticode 证书），首次安装可能触发 SmartScreen 提示。
- **重置机会只读**：Codex 重置机会仅展示数量与到期时间，不支持在应用内兑换。
- **WebView2 毛玻璃限制**：Windows WebView2 对窗口外桌面背景的 CSS `backdrop-filter` 支持有限，当前采用半透明极光圆角卡片以保证 Windows 10/11 下的视觉一致性。
