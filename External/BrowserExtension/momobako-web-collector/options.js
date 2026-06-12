document.getElementById("importConnection").addEventListener("click", async () => {
  const status = document.getElementById("status");
  try {
    const value = JSON.parse(document.getElementById("connectionJson").value);
    if (!value.baseUrl || !value.token) {
      throw new Error("Connection JSON requires baseUrl and token.");
    }
    await chrome.storage.local.set({
      baseUrl: value.baseUrl,
      token: value.token,
    });
    status.textContent = "Connection imported.";
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : String(error);
  }
});
