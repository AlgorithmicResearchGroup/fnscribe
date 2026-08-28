const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

const phaseLabel = document.querySelector("#phase-label");
const message = document.querySelector("#flowbar-message");
const stopButton = document.querySelector("#stop");
const cancelButton = document.querySelector("#cancel");
let snapshotVersion = 0;

const labels = {
  starting: "Starting",
  recording: "Listening",
  transcribing: "Transcribing locally",
  inserting: "Inserting",
};

function render(snapshot) {
  document.body.dataset.phase = snapshot.phase;
  phaseLabel.textContent =
    snapshot.phase === "recording" && snapshot.recording_mode === "hands_free"
      ? "Listening hands-free"
      : labels[snapshot.phase] || "FnScribe";
  message.textContent = snapshot.message;
  stopButton.hidden = snapshot.phase !== "recording";
  stopButton.disabled = false;
  cancelButton.disabled = false;
}

async function refresh() {
  const version = snapshotVersion;
  const snapshot = await invoke("get_snapshot");
  if (version === snapshotVersion) render(snapshot);
}

async function act(command, button) {
  button.disabled = true;
  try {
    await invoke(command);
  } catch {
    button.disabled = false;
  }
}

stopButton.addEventListener("click", () => act("stop_dictation", stopButton));
cancelButton.addEventListener("click", () => act("cancel_dictation", cancelButton));

async function initialize() {
  await listen("snapshot-changed", ({ payload }) => {
    snapshotVersion += 1;
    render(payload);
  });
  await refresh();
}

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) refresh();
});

initialize();
