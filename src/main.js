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

// A small spinning indicator, so a search in progress is visibly working.
function spinner() {
  const element = document.createElement("span");
  element.className = "spinner";
  element.setAttribute("aria-hidden", "true");
  return element;
}

function render(status) {
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
  return item;
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

const mainView = document.getElementById("main-view");
const infoView = document.getElementById("info-view");

function showInfo(about) {
  refreshLoginItem();
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
