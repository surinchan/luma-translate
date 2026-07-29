const { invoke } = window.__TAURI__.core;

document.querySelector("#translate").addEventListener("click", async () => {
  await invoke("translate_selected").catch(() => {});
});
