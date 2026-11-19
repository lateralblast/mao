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

function render(status) {
  current = status;

  const count = status.receivers.filter((r) => r.selected).length;
  if (status.scanning) {
    statusLine.textContent = "Looking for AirPlay receivers…";
  } else if (count === 0) {
    statusLine.textContent = "Off: audio plays on this Mac only";
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
  if (on) selected.add(identifier);
  else selected.delete(identifier);

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

listen("status-changed", (event) => render(event.payload));
listen("popover-opened", async () => {
  await invoke("refresh_receivers");
  await refresh();
});
refresh();
