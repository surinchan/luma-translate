# Luma Translate

<p align="center">
  <img src="src-tauri/icons/icon.png" alt="Luma Translate icon" width="96">
</p>

<p align="center">
  <strong>A lightweight, cross-platform AI selection translator</strong><br>
  Select text in any supported desktop app, click the floating button, and translate with your own OpenAI-compatible LLM API.
</p>

<p align="center">
  <a href="https://github.com/surinchan/luma-translate/actions/workflows/ci.yml"><img src="https://github.com/surinchan/luma-translate/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/surinchan/luma-translate/releases/latest"><img src="https://img.shields.io/github/v/release/surinchan/luma-translate?display_name=tag&sort=semver" alt="Latest release"></a>
  <a href="https://github.com/surinchan/luma-translate/releases"><img src="https://img.shields.io/github/downloads/surinchan/luma-translate/total" alt="Downloads"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/surinchan/luma-translate" alt="MIT License"></a>
</p>

<p align="center">
  <a href="https://github.com/surinchan/luma-translate/releases/latest"><strong>Download the latest release</strong></a>
  ·
  <a href="README.md">简体中文</a>
  ·
  <a href="https://github.com/surinchan/luma-translate/issues">Report an issue</a>
</p>

Luma Translate is a desktop translation app built with **Rust and Tauri 2**. It provides global text-selection translation on Windows, macOS, and Linux and works with OpenAI-compatible `/chat/completions` APIs.

## Download

Download the appropriate installer from the [latest GitHub Release](https://github.com/surinchan/luma-translate/releases/latest):

| Platform | Recommended package | Architecture |
| --- | --- | --- |
| Windows | `x64-setup.exe` or `.msi` | x86_64 |
| macOS | `.dmg` | Apple Silicon and Intel |
| Linux | `.AppImage`, `.deb`, or `.rpm` | x86_64 |

> Windows packages are currently unsigned. macOS packages use ad-hoc signing and are not notarized, so your operating system may display a security warning on first launch.

## Screenshot

<p align="center">
  <img src="docs/images/settings.png" alt="Luma Translate OpenAI-compatible LLM API settings" width="560">
</p>

## Features

- Global text-selection translation across supported desktop applications
- Compact floating translate button that automatically hides when there is no selection
- OpenAI-compatible LLM API and custom model support
- Configurable source and target languages
- Manual text translation for longer content
- Copy translated text, system tray support, and optional launch at login
- Native Rust backend with a lightweight Tauri interface

## Quick start

1. Download and install Luma Translate from the [latest release](https://github.com/surinchan/luma-translate/releases/latest).
2. Open Settings from the system tray.
3. Enter your OpenAI-compatible API endpoint, API key, and model name.
4. Select text in another application and click the translate button.

Example:

```text
API endpoint: https://api.openai.com/v1
Model: gpt-4.1-mini
Source language: Auto detect
Target language: Simplified Chinese
```

The API key is stored only in the current user's local application configuration directory. Selected text is sent to the configured API only after a translation is explicitly requested.

## Development

Requirements:

- Rust stable
- Node.js LTS
- npm

```bash
git clone https://github.com/surinchan/luma-translate.git
cd luma-translate
npm ci
npm run dev
```

Build installers locally with:

```bash
npm run build
```

Bundles are written to `src-tauri/target/release/bundle/`.

## Platform notes

- **Windows:** Run Luma Translate as administrator to capture selections from elevated applications.
- **macOS:** Grant Accessibility permission in System Settings → Privacy & Security → Accessibility.
- **Linux:** Install `xdotool` and use an X11 session. Global input capture is limited under Wayland.

## License

[MIT](LICENSE)
