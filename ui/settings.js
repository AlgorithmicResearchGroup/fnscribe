const invoke = window.__TAURI__.core.invoke;

const statusDot = document.querySelector("#status-dot");
const statusText = document.querySelector("#status-text");
const hotkeyButton = document.querySelector("#hotkey");
const accessibilityButton = document.querySelector("#accessibility");
const microphoneButton = document.querySelector("#microphone");
const errorText = document.querySelector("#error");
const quitButton = document.querySelector("#quit");

let recordingShortcut = false;
let currentHotkey = "Fn";
let hotkeyAtCapture = null;

const keyNames = {
  Space: "Space",
  Enter: "Return",
  Escape: "Esc",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
};

function displayHotkey(value) {
  if (value === "Fn") return "fn";
  const parts = value.split("+");
  const key = parts.pop();
  const symbols = parts
    .map((part) => ({ Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘" })[part] || part)
    .join("");
  const readableKey = keyNames[key] || key.replace(/^Key/, "").replace(/^Digit/, "");
  return `${symbols}${readableKey}`;
}

function shortcutFromEvent(event) {
  if (["Meta", "Shift", "Control", "Alt"].includes(event.key)) return null;

  const parts = [];
  if (event.ctrlKey) parts.push("Control");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Super");

  const functionKey = /^F([1-9]|1[0-9]|2[0-4])$/.test(event.code);
  if (parts.length === 0 && !functionKey) {
    throw new Error("Use at least one modifier key.");
  }

  parts.push(event.code);
  return parts.join("+");
}

async function refresh() {
  try {
    const snapshot = await invoke("get_snapshot");
    if (
      recordingShortcut &&
      (!snapshot.capturing_hotkey ||
        (hotkeyAtCapture && snapshot.hotkey !== hotkeyAtCapture))
    ) {
      recordingShortcut = false;
      hotkeyAtCapture = null;
      hotkeyButton.classList.remove("recording");
      errorText.textContent = "";
    }
    currentHotkey = snapshot.hotkey;
    if (!recordingShortcut) hotkeyButton.textContent = displayHotkey(currentHotkey);

    statusDot.className = `status-dot ${snapshot.phase}`;
    statusText.textContent = snapshot.message;

    const keyboardAccess = snapshot.accessibility_trusted;
    accessibilityButton.textContent = keyboardAccess
      ? "Granted ✓"
      : "Grant permission";
    accessibilityButton.classList.toggle("granted", keyboardAccess);
    accessibilityButton.disabled = keyboardAccess;

    microphoneButton.textContent = snapshot.microphone_trusted
      ? "Granted ✓"
      : "Grant permission";
    microphoneButton.classList.toggle("granted", snapshot.microphone_trusted);
    microphoneButton.disabled = snapshot.microphone_trusted;
  } catch (error) {
    errorText.textContent = String(error);
  }
}

hotkeyButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("begin_hotkey_capture");
    recordingShortcut = true;
    hotkeyAtCapture = currentHotkey;
    hotkeyButton.classList.add("recording");
    hotkeyButton.textContent = "Press keys or fn…";
  } catch (error) {
    recordingShortcut = false;
    hotkeyAtCapture = null;
    hotkeyButton.classList.remove("recording");
    hotkeyButton.textContent = displayHotkey(currentHotkey);
    errorText.textContent = String(error);
  }
});

window.addEventListener("keydown", async (event) => {
  if (!recordingShortcut) return;
  event.preventDefault();
  event.stopPropagation();

  if (event.key === "Escape") {
    recordingShortcut = false;
    hotkeyAtCapture = null;
    await invoke("cancel_hotkey_capture");
    hotkeyButton.classList.remove("recording");
    hotkeyButton.textContent = displayHotkey(currentHotkey);
    return;
  }

  try {
    const shortcut = shortcutFromEvent(event);
    if (!shortcut) return;
    await invoke("set_hotkey", { hotkey: shortcut });
    currentHotkey = shortcut;
    recordingShortcut = false;
    hotkeyAtCapture = null;
    hotkeyButton.classList.remove("recording");
    hotkeyButton.textContent = displayHotkey(shortcut);
    errorText.textContent = "";
  } catch (error) {
    errorText.textContent = String(error).replace(/^Error: /, "");
  }
});

window.addEventListener("blur", () => {
  if (!recordingShortcut) return;
  recordingShortcut = false;
  hotkeyAtCapture = null;
  hotkeyButton.classList.remove("recording");
  hotkeyButton.textContent = displayHotkey(currentHotkey);
  invoke("cancel_hotkey_capture");
});

accessibilityButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("request_keyboard_access");
    window.setTimeout(refresh, 800);
  } catch (error) {
    errorText.textContent = String(error);
  }
});

microphoneButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("request_microphone");
    window.setTimeout(refresh, 800);
  } catch (error) {
    errorText.textContent = String(error);
  }
});

quitButton.addEventListener("click", () => invoke("quit_app"));

refresh();
window.setInterval(refresh, 500);
