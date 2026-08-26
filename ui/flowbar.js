const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

const phaseLabel = document.querySelector("#phase-label");
const message = document.querySelector("#flowbar-message");
const stopButton = document.querySelector("#stop");
const cancelButton = document.querySelector("#cancel");

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

listen("snapshot-changed", ({ payload }) => render(payload));
invoke("get_snapshot").then(render);
