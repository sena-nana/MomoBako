const fields = ["baseUrl", "token", "repoId", "parentPath"];

document.addEventListener("DOMContentLoaded", async () => {
  const values = await chrome.storage.local.get([...fields, "lastResult"]);
  for (const field of fields) {
    document.getElementById(field).value = values[field] || "";
  }
  document.getElementById("status").textContent = values.lastResult || "";
});

document.getElementById("save").addEventListener("click", async () => {
  await chrome.storage.local.set(readForm());
  setStatus("Saved.");
});

document.getElementById("loadRepos").addEventListener("click", async () => {
  const settings = readForm();
  try {
    const response = await fetch(`${settings.baseUrl.replace(/\/+$/, "")}/repositories`, {
      headers: { "Authorization": `Bearer ${settings.token}` },
    });
    const repositories = await response.json();
    if (!response.ok) throw new Error(repositories.message || `HTTP ${response.status}`);
    const select = document.getElementById("repositories");
    select.textContent = "";
    for (const repo of repositories) {
      const option = document.createElement("option");
      option.value = repo.repoId;
      option.textContent = `${repo.name} (${repo.repoId})`;
      select.append(option);
    }
    if (repositories[0]) {
      document.getElementById("repoId").value = repositories[0].repoId;
    }
    setStatus(`Loaded ${repositories.length} repository(s).`);
  } catch (error) {
    setStatus(error instanceof Error ? error.message : String(error));
  }
});

document.getElementById("repositories").addEventListener("change", (event) => {
  document.getElementById("repoId").value = event.target.value;
});

function readForm() {
  return Object.fromEntries(fields.map((field) => [field, document.getElementById(field).value.trim()]));
}

function setStatus(value) {
  document.getElementById("status").textContent = value;
}
