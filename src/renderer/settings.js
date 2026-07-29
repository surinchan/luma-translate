const { invoke } = window.__TAURI__.core;
const form = document.querySelector("#form");
const saveButton = document.querySelector("#save");
const status = document.querySelector("#status");

async function load() {
  const settings = await invoke("get_settings");
  for (const key of ["endpoint", "model", "sourceLanguage", "targetLanguage"]) {
    document.querySelector(`#${key}`).value = settings[key];
  }
  document.querySelector("#launchAtLogin").checked = settings.launchAtLogin;
  if (settings.hasApiKey) {
    document.querySelector("#keyHint").textContent = "已保存 API Key；保持输入框留空则不修改。";
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  saveButton.disabled = true;
  status.textContent = "";
  const data = new FormData(form);
  const payload = {
    endpoint: data.get("endpoint"),
    apiKey: data.get("apiKey"),
    model: data.get("model"),
    sourceLanguage: data.get("sourceLanguage"),
    targetLanguage: data.get("targetLanguage"),
    launchAtLogin: data.get("launchAtLogin") === "on"
  };
  try {
    await invoke("set_settings", { input: payload });
    document.querySelector("#apiKey").value = "";
    document.querySelector("#keyHint").textContent = "已保存 API Key；保持输入框留空则不修改。";
    status.textContent = "设置已保存";
    setTimeout(() => { status.textContent = ""; }, 2200);
  } catch (error) {
    status.style.color = "#b83b4b";
    status.textContent = error.message;
  } finally {
    saveButton.disabled = false;
  }
});

load();
