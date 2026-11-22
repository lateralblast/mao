const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const list = document.getElementById("receivers");
const statusLine = document.getElementById("status");
const errorLine = document.getElementById("error");

const STATE_LABELS = {
  connecting: "connecting",
  streaming: "streaming",
  ended: "ended",
  error: "error",
  unavailable: "not on the network",
};

let current = null;
let busy = false;
// While a volume slider is being dragged, the list is not rebuilt, so the slider is not lost.
let dragging = false;
let pendingStatus = null;

// A small spinning indicator, so a search in progress is visibly working.
function spinner() {
  const element = document.createElement("span");
  element.className = "spinner";
  element.setAttribute("aria-hidden", "true");
  return element;
}

function renderMuteAll(allMuted) {
  const button = document.getElementById("mute-all");
  button.replaceChildren(speakerIcon(allMuted));
  button.setAttribute("aria-pressed", String(allMuted));
  button.title = allMuted ? "Unmute all (Option+Command+M)" : "Mute all (Option+Command+M)";
  button.setAttribute("aria-label", button.title);
}

function render(status) {
  renderMuteAll(status.allMuted);
  if (dragging) {
    pendingStatus = status;
    return;
  }
  current = status;

  const count = status.receivers.filter((r) => r.selected).length;
  if (status.scanning) {
    statusLine.replaceChildren(spinner(), document.createTextNode("Looking for AirPlay receivers…"));
  } else if (count === 0) {
    statusLine.textContent = "Muted: nothing ticked, so no audio plays";
  } else {
    statusLine.textContent = `Sending to ${count} receiver${count === 1 ? "" : "s"}`;
  }

  errorLine.hidden = !status.error;
  errorLine.textContent = status.error ?? "";

  if (status.receivers.length === 0) {
    const empty = document.createElement("li");
    empty.className = "empty";
    empty.textContent = status.scanning ? "Searching…" : "No AirPlay receivers found";
    list.replaceChildren(empty);
    return;
  }

  list.replaceChildren(...status.receivers.map(renderRow));
}

function renderRow(receiver) {
  const item = document.createElement("li");
  if (!receiver.available) item.className = "unavailable";

  const label = document.createElement("label");

  const box = document.createElement("input");
  box.type = "checkbox";
  box.checked = receiver.selected;
  box.disabled = busy;
  box.addEventListener("change", () => toggle(receiver.identifier, box.checked));

  const name = document.createElement("span");
  name.className = "name";
  name.textContent = receiver.name;
  name.title = receiver.name;

  label.append(box, name);

  if (receiver.kind === "local") {
    const place = document.createElement("span");
    place.className = "tag";
    place.textContent = "this Mac";
    label.append(place);
  }

  if (receiver.selected && receiver.state !== "idle") {
    const tag = document.createElement("span");
    tag.className = `tag state-${receiver.state}`;
    tag.textContent = STATE_LABELS[receiver.state] ?? receiver.state;
    if (receiver.message) tag.title = receiver.message;
    label.append(tag);
  }

  item.append(label);
  if (receiver.selected && receiver.state === "streaming" && typeof receiver.volume === "number") {
    item.append(volumeRow(receiver));
  }
  return item;
}

// Speaker icon for the mute button: a speaker with sound waves, or with a cross when muted.
function speakerIcon(muted) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("aria-hidden", "true");
  const body = document.createElementNS(ns, "path");
  body.setAttribute("d", "M2 6h2.5L8 3.2v9.6L4.5 10H2z");
  body.setAttribute("fill", "currentColor");
  svg.append(body);
  const shape = document.createElementNS(ns, "path");
  shape.setAttribute("fill", "none");
  shape.setAttribute("stroke", "currentColor");
  shape.setAttribute("stroke-width", "1.4");
  shape.setAttribute("stroke-linecap", "round");
  if (muted) {
    shape.setAttribute("d", "M10.5 6l3.5 4M14 6l-3.5 4");
  } else {
    shape.setAttribute("d", "M10.5 5.5a3.5 3.5 0 0 1 0 5M12.5 3.5a6 6 0 0 1 0 9");
  }
  svg.append(shape);
  return svg;
}

function volumeRow(receiver) {
  const row = document.createElement("div");
  row.className = "volume";

  const mute = document.createElement("button");
  mute.type = "button";
  mute.className = "mute";
  mute.title = receiver.muted ? "Unmute" : "Mute";
  mute.setAttribute("aria-label", `${receiver.muted ? "Unmute" : "Mute"} ${receiver.name}`);
  mute.setAttribute("aria-pressed", String(receiver.muted));
  mute.append(speakerIcon(receiver.muted));
  mute.addEventListener("click", async () => {
    try {
      const muted = !receiver.muted;
      if (receiver.kind === "local") {
        await invoke("set_local_mute", { identifier: receiver.identifier, muted });
      } else {
        await invoke("set_mute", { identifier: receiver.identifier, muted });
      }
    } catch (err) {
      errorLine.hidden = false;
      errorLine.textContent = String(err);
    }
  });

  const slider = document.createElement("input");
  slider.type = "range";
  slider.min = "0";
  slider.max = "100";
  slider.step = "1";
  slider.value = String(Math.round(receiver.volume));
  slider.setAttribute("aria-label", `Volume for ${receiver.name}`);
  // While muted, the slider keeps its place but cannot be moved; unmuting restores the level.
  slider.disabled = receiver.muted;

  const level = document.createElement("span");
  level.className = "level";
  level.textContent = `${Math.round(receiver.volume)}%`;

  slider.addEventListener("pointerdown", () => {
    dragging = true;
  });
  slider.addEventListener("input", () => {
    level.textContent = `${slider.value}%`;
  });
  slider.addEventListener("change", async () => {
    try {
      const level = Number(slider.value);
      if (receiver.kind === "local") {
        await invoke("set_local_volume", { identifier: receiver.identifier, level });
      } else {
        await invoke("set_volume", { identifier: receiver.identifier, level });
      }
    } catch (err) {
      errorLine.hidden = false;
      errorLine.textContent = String(err);
    }
    endDrag();
  });
  // A press without a change (no movement) still has to end the drag.
  slider.addEventListener("pointerup", () => setTimeout(endDrag, 300));

  row.append(mute, slider, level);
  return row;
}

function endDrag() {
  dragging = false;
  if (pendingStatus) {
    const status = pendingStatus;
    pendingStatus = null;
    render(status);
  }
}

async function toggle(identifier, on) {
  const selected = new Set(current ? current.receivers.filter((r) => r.selected).map((r) => r.identifier) : []);
  if (on) {
    // Only one output on this Mac can be the system output, so ticking one unticks the others.
    if (identifier.startsWith("local:")) {
      for (const id of [...selected]) {
        if (id.startsWith("local:")) selected.delete(id);
      }
    }
    selected.add(identifier);
  } else {
    selected.delete(identifier);
  }

  busy = true;
  try {
    render(await invoke("set_selected", { identifiers: [...selected] }));
  } catch (err) {
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  } finally {
    busy = false;
    await refresh();
  }
}

async function refresh() {
  try {
    render(await invoke("get_status"));
  } catch (err) {
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  }
}

document.getElementById("quit").addEventListener("click", () => invoke("quit_app"));
document.getElementById("mute-all").addEventListener("click", async () => {
  try {
    await invoke("toggle_mute_all");
  } catch (err) {
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  }
});

const mainView = document.getElementById("main-view");
const infoView = document.getElementById("info-view");

function showInfo(about) {
  refreshLoginItem();
  refreshNotifications();
  refreshRemember();
  document.getElementById("version").textContent = `Version ${about.version}`;
  document.getElementById("help-title").textContent = about.help_title;
  renderHelp(about.help);
  renderBlock("attribution", about.attribution);
  mainView.hidden = true;
  infoView.hidden = false;
}

function showMain() {
  infoView.hidden = true;
  mainView.hidden = false;
}

async function openInfo() {
  try {
    showInfo(await invoke("about"));
  } catch (err) {
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  }
}

// Clicking anywhere in the info view returns to the main view.
infoView.addEventListener("click", showMain);

// Clicks on the login setting change it, so they must not also return to the main view.
const loginSetting = document.getElementById("login-setting");
const loginToggle = document.getElementById("login-toggle");
const loginNote = document.getElementById("login-note");
loginSetting.addEventListener("click", (event) => event.stopPropagation());

const notifySetting = document.getElementById("notify-setting");
const notifyToggle = document.getElementById("notify-toggle");
notifySetting.addEventListener("click", (event) => event.stopPropagation());

async function refreshNotifications() {
  try {
    notifyToggle.checked = await invoke("get_notify_on_drop");
  } catch (err) {
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  }
}

const rememberSetting = document.getElementById("remember-setting");
const rememberToggle = document.getElementById("remember-toggle");
rememberSetting.addEventListener("click", (event) => event.stopPropagation());

async function refreshRemember() {
  try {
    rememberToggle.checked = await invoke("get_remember_receivers");
  } catch (err) {
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  }
}

rememberToggle.addEventListener("change", async () => {
  try {
    rememberToggle.checked = await invoke("set_remember_receivers", { enabled: rememberToggle.checked });
  } catch (err) {
    rememberToggle.checked = !rememberToggle.checked;
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  }
});

notifyToggle.addEventListener("change", async () => {
  try {
    notifyToggle.checked = await invoke("set_notify_on_drop", { enabled: notifyToggle.checked });
  } catch (err) {
    notifyToggle.checked = !notifyToggle.checked;
    errorLine.hidden = false;
    errorLine.textContent = String(err);
  }
});

async function refreshLoginItem() {
  try {
    const item = await invoke("login_item");
    loginToggle.checked = item.enabled;
    loginToggle.disabled = !item.available;
    loginNote.hidden = item.available;
    loginNote.textContent = item.available ? "" : "Available in the installed app.";
  } catch (err) {
    loginNote.hidden = false;
    loginNote.textContent = String(err);
  }
}

loginToggle.addEventListener("change", async () => {
  loginNote.hidden = true;
  try {
    const item = await invoke("set_login_item", { enabled: loginToggle.checked });
    loginToggle.checked = item.enabled;
  } catch (err) {
    loginToggle.checked = !loginToggle.checked;
    loginNote.hidden = false;
    loginNote.textContent = String(err);
  }
});
document.getElementById("info-open").addEventListener("click", openInfo);

// Turns a README section into paragraphs and bullet lists, without Markdown markup.
function renderHelp(text) {
  renderBlock("help", text);
}

function renderBlock(id, text) {
  const nodes = [];
  let list = null;
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (line === "") {
      list = null;
      continue;
    }
    if (line.startsWith("- ")) {
      if (!list) {
        list = document.createElement("ul");
        nodes.push(list);
      }
      const item = document.createElement("li");
      appendWithLinks(item, plain(line.slice(2)));
      list.append(item);
      continue;
    }
    list = null;
    const paragraph = document.createElement("p");
    appendWithLinks(paragraph, plain(line));
    nodes.push(paragraph);
  }
  document.getElementById(id).replaceChildren(...nodes);
}

// Adds text to `element`, turning any web address into a link that opens in the browser.
function appendWithLinks(element, text) {
  for (const part of text.split(/(https?:\/\/\S+)/)) {
    if (/^https?:\/\//.test(part)) {
      const link = document.createElement("a");
      link.href = part;
      link.textContent = part;
      link.addEventListener("click", (event) => {
        // Keep the info view open: only the link is followed.
        event.preventDefault();
        event.stopPropagation();
        invoke("open_url", { url: part }).catch((err) => {
          errorLine.hidden = false;
          errorLine.textContent = String(err);
        });
      });
      element.append(link);
    } else if (part !== "") {
      element.append(document.createTextNode(part));
    }
  }
}

function plain(markdown) {
  return markdown
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, "$1 ($2)")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/\*([^*]+)\*/g, "$1");
}

listen("status-changed", (event) => render(event.payload));
listen("popover-opened", async () => {
  showMain();
  await invoke("refresh_receivers");
  await refresh();
});
refresh();
