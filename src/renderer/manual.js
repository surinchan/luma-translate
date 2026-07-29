const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const source = document.querySelector("#manual-source");
const result = document.querySelector("#manual-result");
const status = document.querySelector("#manual-status");
const characterCount = document.querySelector("#character-count");
const targetLanguage = document.querySelector("#manual-target");
const translateButton = document.querySelector("#translate-text");
const copyButton = document.querySelector("#copy");
const clearButton = document.querySelector("#clear");
let translatedText = "";
let translating = false;

async function loadTargetLanguage() {
  try {
    const settings = await invoke("get_settings");
    const configured = settings.targetLanguage;
    if (![...targetLanguage.options].some((option) => option.value === configured)) {
      targetLanguage.add(new Option(configured, configured));
    }
    targetLanguage.value = configured;
  } catch {
    // Keep the first built-in language available if settings cannot be read.
  }
}

function updateCount() {
  characterCount.textContent = `${Array.from(source.value).length} / 50000`;
}

function setResult(text, className = "") {
  result.className = `manual-result${className ? ` ${className}` : ""}`;
  result.textContent = text;
}

async function runTranslation() {
  const text = source.value.trim();
  if (!text) {
    setResult("请输入需要翻译的文本", "error");
    status.textContent = "没有可翻译的内容";
    source.focus();
    return;
  }
  if (translating) return;

  translating = true;
  translatedText = "";
  translateButton.disabled = true;
  copyButton.disabled = true;
  setResult("正在翻译…", "loading-text");
  status.textContent = "正在调用 LLM 服务";

  try {
    translatedText = await invoke("translate_text", {
      text,
      targetLanguage: targetLanguage.value
    });
    setResult(translatedText);
    copyButton.disabled = false;
    status.textContent = "翻译完成";
  } catch (error) {
    const message = String(error);
    setResult(message, "error");
    status.textContent = "翻译失败";
    if (message.includes("API Key")) {
      const settingsButton = document.createElement("button");
      settingsButton.className = "inline-link";
      settingsButton.type = "button";
      settingsButton.textContent = "打开设置";
      settingsButton.addEventListener("click", () => invoke("open_settings"));
      result.append(document.createElement("br"), settingsButton);
    }
  } finally {
    translating = false;
    translateButton.disabled = false;
  }
}

source.addEventListener("input", updateCount);
source.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
    event.preventDefault();
    runTranslation();
  }
});

translateButton.addEventListener("click", runTranslation);
copyButton.addEventListener("click", async () => {
  if (!translatedText) return;
  await invoke("copy_translation", { text: translatedText });
  status.textContent = "译文已复制";
});
clearButton.addEventListener("click", () => {
  source.value = "";
  translatedText = "";
  updateCount();
  copyButton.disabled = true;
  setResult("译文将在这里显示", "placeholder");
  status.textContent = "Ctrl + Enter 快速翻译";
  source.focus();
});
document.querySelector("#settings").addEventListener("click", () => invoke("open_settings"));

listen("manual-window-opened", () => {
  loadTargetLanguage().finally(() => requestAnimationFrame(() => source.focus()));
});

window.addEventListener("DOMContentLoaded", () => {
  updateCount();
  loadTargetLanguage();
  source.focus();
});
