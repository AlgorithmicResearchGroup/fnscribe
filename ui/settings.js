const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

const statusDot = document.querySelector("#status-dot");
const statusText = document.querySelector("#status-text");
const hotkeyButton = document.querySelector("#hotkey");
const handsFreeButton = document.querySelector("#hands-free");
const microphoneSelect = document.querySelector("#microphone-select");
const microphoneDetail = document.querySelector("#microphone-detail");
const launchAtLogin = document.querySelector("#launch-at-login");
const smartCleanup = document.querySelector("#smart-cleanup");
const dictionaryDetail = document.querySelector("#dictionary-detail");
const openDictionaryButton = document.querySelector("#open-dictionary");
const accessibilityButton = document.querySelector("#accessibility");
const microphoneButton = document.querySelector("#microphone");
const copyOriginalButton = document.querySelector("#copy-original");
const copyLastButton = document.querySelector("#copy-last");
const pasteLastButton = document.querySelector("#paste-last");
const errorText = document.querySelector("#error");
const quitButton = document.querySelector("#quit");
const dictionaryDialog = document.querySelector("#dictionary-dialog");
const closeDictionaryButton = document.querySelector("#close-dictionary");
const dictionaryForm = document.querySelector("#dictionary-form");
const dictionaryWritten = document.querySelector("#dictionary-written");
const dictionarySpoken = document.querySelector("#dictionary-spoken");
const saveDictionaryButton = document.querySelector("#save-dictionary-entry");
const cancelDictionaryEdit = document.querySelector("#cancel-dictionary-edit");
const dictionaryError = document.querySelector("#dictionary-error");
const dictionaryList = document.querySelector("#dictionary-list");

let recordingShortcut = false;
let currentHotkey = "Fn";
let hotkeyAtCapture = null;
let selectedMicrophoneId = null;
let editingWrittenForm = null;

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

function cleanError(error) {
  return String(error).replace(/^Error: /, "");
}

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

function dictionaryCountLabel(count) {
  if (count === 0) return "No saved terms";
  return `${count} saved ${count === 1 ? "term" : "terms"}`;
}

function resetDictionaryForm() {
  editingWrittenForm = null;
  dictionaryForm.reset();
  saveDictionaryButton.textContent = "Add term";
  cancelDictionaryEdit.hidden = true;
  dictionaryError.textContent = "";
}

function beginDictionaryEdit(entry) {
  editingWrittenForm = entry.written_form;
  dictionaryWritten.value = entry.written_form;
  dictionarySpoken.value = entry.spoken_form || "";
  saveDictionaryButton.textContent = "Save changes";
  cancelDictionaryEdit.hidden = false;
  dictionaryError.textContent = "";
  dictionaryWritten.focus();
  dictionaryWritten.select();
}

function renderDictionaryEntries(entries) {
  dictionaryDetail.textContent = dictionaryCountLabel(entries.length);
  dictionaryList.replaceChildren();

  if (entries.length === 0) {
    const empty = document.createElement("p");
    empty.className = "dictionary-empty";
    empty.textContent = "Add names, product terms, acronyms, or recurring corrections.";
    dictionaryList.append(empty);
    return;
  }

  for (const entry of entries) {
    const row = document.createElement("article");
    row.className = "dictionary-entry";

    const copy = document.createElement("div");
    const written = document.createElement("strong");
    written.textContent = entry.written_form;
    copy.append(written);
    if (entry.spoken_form) {
      const spoken = document.createElement("span");
      spoken.textContent = `Replace “${entry.spoken_form}”`;
      copy.append(spoken);
    } else {
      const hint = document.createElement("span");
      hint.textContent = "Recognition hint and exact casing";
      copy.append(hint);
    }

    const actions = document.createElement("div");
    actions.className = "dictionary-entry-actions";
    const edit = document.createElement("button");
    edit.className = "link-button";
    edit.type = "button";
    edit.textContent = "Edit";
    edit.addEventListener("click", () => beginDictionaryEdit(entry));
    const remove = document.createElement("button");
    remove.className = "link-button destructive";
    remove.type = "button";
    remove.textContent = "Delete";
    let confirmingDelete = false;
    remove.addEventListener("click", async () => {
      if (!confirmingDelete) {
        confirmingDelete = true;
        remove.textContent = "Confirm";
        remove.setAttribute("aria-label", `Confirm deleting ${entry.written_form}`);
        window.setTimeout(() => {
          confirmingDelete = false;
          remove.textContent = "Delete";
          remove.removeAttribute("aria-label");
        }, 4000);
        return;
      }
      dictionaryError.textContent = "";
      try {
        const updated = await invoke("delete_dictionary_entry", {
          writtenForm: entry.written_form,
        });
        if (editingWrittenForm === entry.written_form) resetDictionaryForm();
        renderDictionaryEntries(updated);
      } catch (error) {
        dictionaryError.textContent = cleanError(error);
      }
    });
    actions.append(edit, remove);
    row.append(copy, actions);
    dictionaryList.append(row);
  }
}

async function loadDictionary() {
  try {
    renderDictionaryEntries(await invoke("get_dictionary_entries"));
  } catch (error) {
    dictionaryError.textContent = cleanError(error);
  }
}

function render(snapshot) {
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
  selectedMicrophoneId = snapshot.microphone_id;
  if (!recordingShortcut) hotkeyButton.textContent = displayHotkey(currentHotkey);

  statusDot.className = `status-dot ${snapshot.phase}`;
  statusText.textContent = snapshot.message;

  const isRecording = snapshot.phase === "recording";
  const isHandsFree = isRecording && snapshot.recording_mode === "hands_free";
  const isPushToTalk = isRecording && snapshot.recording_mode === "push_to_talk";
  const dictationBusy = ["starting", "transcribing", "inserting"].includes(snapshot.phase);
  const sessionActive = dictationBusy || isRecording;
  handsFreeButton.textContent = isHandsFree
    ? "Stop"
    : isPushToTalk
      ? "Keep listening"
      : "Start";
  handsFreeButton.classList.toggle("stop", isHandsFree);
  handsFreeButton.disabled = dictationBusy || snapshot.phase === "loading";

  const keyboardAccess = snapshot.accessibility_trusted;
  accessibilityButton.textContent = keyboardAccess ? "Granted ✓" : "Grant permission";
  accessibilityButton.classList.toggle("granted", keyboardAccess);
  accessibilityButton.disabled = keyboardAccess;

  microphoneButton.textContent = snapshot.microphone_trusted
    ? "Granted ✓"
    : "Grant permission";
  microphoneButton.classList.toggle("granted", snapshot.microphone_trusted);
  microphoneButton.disabled = snapshot.microphone_trusted;

  launchAtLogin.checked = snapshot.launch_at_login;
  smartCleanup.checked = snapshot.smart_cleanup;
  dictionaryDetail.textContent = dictionaryCountLabel(snapshot.dictionary_count);
  copyOriginalButton.disabled = !snapshot.has_original_transcript;
  copyLastButton.disabled = !snapshot.has_last_transcript;
  pasteLastButton.disabled = !snapshot.has_last_transcript || sessionActive;

  const optionExists = [...microphoneSelect.options].some(
    (option) => option.value === (selectedMicrophoneId || ""),
  );
  if (optionExists) microphoneSelect.value = selectedMicrophoneId || "";
}

async function refresh() {
  try {
    render(await invoke("get_snapshot"));
  } catch (error) {
    errorText.textContent = cleanError(error);
  }
}

async function loadMicrophones() {
  try {
    const microphones = await invoke("get_microphones");
    microphoneSelect.replaceChildren();

    const systemDefault = document.createElement("option");
    systemDefault.value = "";
    const defaultMicrophone = microphones.find((microphone) => microphone.is_default);
    systemDefault.textContent = defaultMicrophone
      ? `System default — ${defaultMicrophone.name}`
      : "System default";
    microphoneSelect.append(systemDefault);

    for (const microphone of microphones) {
      const option = document.createElement("option");
      option.value = microphone.id;
      option.textContent = microphone.name;
      microphoneSelect.append(option);
    }

    microphoneSelect.value = selectedMicrophoneId || "";
    const selectedExists = microphoneSelect.value === (selectedMicrophoneId || "");
    microphoneDetail.textContent = selectedMicrophoneId
      ? selectedExists
        ? "Always use this input when available"
        : "Saved input unavailable; using system default"
      : "Follows the macOS default input";
  } catch (error) {
    microphoneSelect.disabled = true;
    microphoneDetail.textContent = "Could not list inputs";
    errorText.textContent = cleanError(error);
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
    errorText.textContent = cleanError(error);
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
    errorText.textContent = cleanError(error);
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

handsFreeButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("toggle_hands_free");
  } catch (error) {
    errorText.textContent = cleanError(error);
  }
});

microphoneSelect.addEventListener("change", async () => {
  const previous = selectedMicrophoneId || "";
  const microphoneId = microphoneSelect.value || null;
  microphoneSelect.disabled = true;
  errorText.textContent = "";
  try {
    await invoke("set_microphone", { microphoneId });
    selectedMicrophoneId = microphoneId;
    microphoneDetail.textContent = microphoneId
      ? "Always use this input when available"
      : "Follows the macOS default input";
  } catch (error) {
    microphoneSelect.value = previous;
    errorText.textContent = cleanError(error);
  } finally {
    microphoneSelect.disabled = false;
  }
});

launchAtLogin.addEventListener("change", async () => {
  const enabled = launchAtLogin.checked;
  launchAtLogin.disabled = true;
  errorText.textContent = "";
  try {
    await invoke("set_launch_at_login", { enabled });
  } catch (error) {
    launchAtLogin.checked = !enabled;
    errorText.textContent = cleanError(error);
  } finally {
    launchAtLogin.disabled = false;
  }
});

smartCleanup.addEventListener("change", async () => {
  const enabled = smartCleanup.checked;
  smartCleanup.disabled = true;
  errorText.textContent = "";
  try {
    await invoke("set_smart_cleanup", { enabled });
  } catch (error) {
    smartCleanup.checked = !enabled;
    errorText.textContent = cleanError(error);
  } finally {
    smartCleanup.disabled = false;
  }
});

openDictionaryButton.addEventListener("click", async () => {
  resetDictionaryForm();
  if (!dictionaryDialog.open) dictionaryDialog.showModal();
  await loadDictionary();
  dictionaryWritten.focus();
});

closeDictionaryButton.addEventListener("click", () => {
  dictionaryDialog.close();
});

dictionaryDialog.addEventListener("close", resetDictionaryForm);

cancelDictionaryEdit.addEventListener("click", () => {
  resetDictionaryForm();
  dictionaryWritten.focus();
});

dictionaryForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  dictionaryError.textContent = "";
  saveDictionaryButton.disabled = true;
  try {
    const updated = await invoke("save_dictionary_entry", {
      originalWrittenForm: editingWrittenForm,
      writtenForm: dictionaryWritten.value,
      spokenForm: dictionarySpoken.value || null,
    });
    renderDictionaryEntries(updated);
    resetDictionaryForm();
    dictionaryWritten.focus();
  } catch (error) {
    dictionaryError.textContent = cleanError(error);
  } finally {
    saveDictionaryButton.disabled = false;
  }
});

accessibilityButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("request_keyboard_access");
    window.setTimeout(refresh, 800);
  } catch (error) {
    errorText.textContent = cleanError(error);
  }
});

microphoneButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("request_microphone");
    window.setTimeout(async () => {
      await refresh();
      await loadMicrophones();
    }, 800);
  } catch (error) {
    errorText.textContent = cleanError(error);
  }
});

copyLastButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("copy_last_transcript");
  } catch (error) {
    errorText.textContent = cleanError(error);
  }
});

copyOriginalButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("copy_original_transcript");
  } catch (error) {
    errorText.textContent = cleanError(error);
  }
});

pasteLastButton.addEventListener("click", async () => {
  errorText.textContent = "";
  try {
    await invoke("paste_last_transcript");
  } catch (error) {
    errorText.textContent = cleanError(error);
  }
});

quitButton.addEventListener("click", () => invoke("quit_app"));

listen("snapshot-changed", ({ payload }) => render(payload));
window.addEventListener("focus", async () => {
  await refresh();
  await loadMicrophones();
});

refresh().then(loadMicrophones);
