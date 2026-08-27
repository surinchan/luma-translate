const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const source = document.querySelector("#source");
const result = document.querySelector("#result");
const toast = document.querySelector("#toast");
let translatedText = "";

listen("selection-changed", ({ payload }) => {
  source.textContent = payload;
});

listen("translation-loading", () => {
  translatedText = "";
  result.className = "result loading";
  result.innerHTML = '<span class="dots"><i></i><i></i><i></i></span><span>正在理解并翻译…</span>';
});

listen("translation-finished", ({ payload }) => {
  translatedText = payload;
  result.className = "result";
  result.textContent = payload;
});

listen("translation-error", ({ payload }) => {
  translatedText = "";
  result.className = "result error";
  result.textContent = payload;
  if (String(payload).includes("API Key")) {
    const button = document.createElement("button");
    button.textContent = "打开设置";
    button.addEventListener("click", () => invoke("open_settings"));
    result.append(document.createElement("br"), button);
  }
});

document.querySelector("#copy").addEventListener("click", async () => {
  if (!translatedText) return;
  await invoke("copy_translation", { text: translatedText });
  toast.classList.add("show");
  setTimeout(() => toast.classList.remove("show"), 1200);
});
document.querySelector("#settings").addEventListener("click", () => invoke("open_settings"));
document.querySelector("#close").addEventListener("click", () => invoke("close_panel"));
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    invoke("close_panel");
    return;
  }
  if ((event.ctrlKey || event.metaKey) && ["+", "-", "=", "0"].includes(event.key)) {
    event.preventDefault();
  }
});

document.addEventListener("wheel", (event) => {
  if (event.ctrlKey || event.metaKey) event.preventDefault();
}, { passive: false });

window.addEventListener("DOMContentLoaded", () => {
  translatedText = "";
  result.className = "result loading";
  result.innerHTML = '<span class="dots"><i></i><i></i><i></i></span><span>正在理解并翻译…</span>';
});
