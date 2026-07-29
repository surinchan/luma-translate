# Luma Translate

[![CI](https://github.com/surinchan/luma-translate/actions/workflows/ci.yml/badge.svg)](https://github.com/surinchan/luma-translate/actions/workflows/ci.yml)

一个使用 **Rust + Tauri 2** 实现的跨平台桌面划词翻译工具。鼠标选中文字后，选区旁会出现翻译按钮；点击后通过 OpenAI 兼容的 LLM API 翻译并显示结果。

## 应用截图

<p align="center">
  <img src="docs/images/settings.png" alt="Luma Translate 设置窗口" width="560">
</p>

<p align="center"><sub>配置 OpenAI 兼容 API、模型、翻译语言和开机启动。</sub></p>

## 功能

- Windows、macOS、Linux 全局划词检测
- 跟随鼠标出现的轻量翻译按钮
- 托盘菜单打开输入窗口，支持直接输入或粘贴文本翻译
- 手动翻译可临时选择目标语言，不修改划词翻译的默认语言
- OpenAI 兼容 `/chat/completions` 接口
- 可配置 API 地址、API Key、模型和源/目标语言
- 译文一键复制、托盘常驻、可选开机启动

## 开发运行

```bash
rustup update stable
npm install
npm run dev
```

首次启动后，通过托盘图标打开设置，填写：

- API 地址，例如 `https://api.openai.com/v1`
- API Key
- 模型名称，例如 `gpt-4.1-mini`

随后在其他应用中按住鼠标拖动选中文字，松开鼠标即可看到翻译按钮。

## 打包

```bash
npm run build
```

安装包会生成在 `src-tauri/target/release/bundle/`。

## 自动发布

推送到 `main` 或创建 Pull Request 时，GitHub Actions 会在 Windows、Linux 和 macOS 上执行语法检查、格式检查与 Rust 测试。

发布新版本前，先确保 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 中的版本号一致，然后推送与版本号相同的标签：

```bash
git tag v0.6.4
git push origin v0.6.4
```

GitHub Actions 会自动创建 GitHub Release，并附上 Windows、Linux、macOS Intel 和 macOS Apple Silicon 的安装包。当前 Windows 安装包未进行商业证书签名，macOS 使用临时签名且未公证，因此首次运行时系统可能显示安全提示。

## 系统权限

- **Windows**：普通应用可直接使用。若要在管理员权限应用中划词，Luma Translate 也需以管理员身份运行。
- **macOS**：首次使用时，需要在“系统设置 → 隐私与安全性 → 辅助功能”中允许本应用控制键盘。
- **Linux**：需要安装 `xdotool`，并在 X11 会话中运行。Wayland 对全局输入捕获有限制，建议切换到 X11 会话。

## 工作原理与隐私

Windows 版本通过低级鼠标钩子识别拖动选区，并使用兼容不同桌面框架的系统复制机制读取文本。程序会优先等待用户自己的 Ctrl+C，并在检测到真实键盘输入时取消自动复制，避免干扰正常快捷键。普通单击或没有产生新复制内容时不会显示翻译按钮；鼠标再次点击或超时后按钮会立即隐藏。macOS 和 Linux 目前也使用系统复制机制。只有点击翻译按钮或在输入翻译窗口主动提交后，文本才会发送到你配置的 LLM API。API 配置保存在当前用户的应用配置目录中，不会写入项目目录。

Windows 捕获链路会把不含正文的诊断信息写入 `%LOCALAPPDATA%\com.luma.selection-translator\logs\selection.log`，仅记录捕获方式、字符数和失败阶段，便于排查特定应用的兼容问题。
