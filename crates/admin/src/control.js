const select = (selector) => document.querySelector(selector);
const apiPath = "/api/admin/v1";
const current = {
  matches: {history: [], cursor: null, next: null},
  players: {history: [], cursor: null, next: null},
  match: null,
  eventAfter: 0,
  generation: 0,
  detailTimer: null,
};
const notice = select("#notice");
const date = (value) => value ? new Date(value).toLocaleString() : "—";
const title = (value) => value ? value.replaceAll("_", " ") : "—";
const yes = (value) => value ? "Yes" : "No";

function report(message = "") {
  notice.textContent = message;
  notice.hidden = !message;
}

async function api(path, options = {}) {
  const headers = options.body ? {"Content-Type": "application/json"} : {};
  const response = await fetch(apiPath + path, {...options, headers, credentials: "same-origin"});
  const data = response.status === 204 ? null : await response.json();
  if (!response.ok) {
    const error = new Error(data.title ?? "Request failed");
    error.status = response.status;
    error.retry = response.headers.get("Retry-After");
    throw error;
  }
  return data;
}

function signedIn(active) {
  select("#sign-in").hidden = active;
  select("#console").hidden = !active;
  select("#sign-out").hidden = !active;
  if (!active) closeMatch();
}

function fail(error) {
  if (error.status === 401) signedIn(false);
  const delay = error.retry ? ` Try again in ${error.retry} seconds.` : "";
  report(error.message + delay);
}

function cellButton(text, action) {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "link";
  button.textContent = text;
  button.addEventListener("click", () => action().catch(fail));
  return button;
}

function rows(selector, values, count, empty) {
  const target = select(selector);
  const body = document.createDocumentFragment();
  if (!values.length) {
    const row = document.createElement("tr");
    const cell = document.createElement("td");
    cell.colSpan = count;
    cell.textContent = empty;
    row.append(cell);
    body.append(row);
  }
  for (const entries of values) {
    const row = document.createElement("tr");
    for (const entry of entries) {
      const cell = document.createElement("td");
      cell.append(entry instanceof Node ? entry : String(entry ?? "—"));
      row.append(cell);
    }
    body.append(row);
  }
  target.replaceChildren(body);
}

async function overview() {
  const data = await api("/overview");
  select("#server-version").textContent =
    `Version ${data.serverVersion} · Client build ${data.clientBuild} · Started ${date(data.startedAt)}`;
  select("#online-players").textContent = data.onlinePlayers;
  select("#queued-players").textContent = data.queuedPlayers;
  select("#active-matches").textContent = data.activeMatches;
}

async function pools() {
  const data = await api("/federation/pools");
  rows("#pool-rows", data.items.map((pool) => [
    pool.poolId, title(pool.state), pool.localRole, pool.voterCount, pool.eligibleHosts, pool.localTickets,
  ]), 6, "No federation pools configured.");
}

async function navigate(kind, direction) {
  const page = current[kind];
  if (direction === "reset") {
    page.history = [];
    page.cursor = null;
  } else if (direction === "next" && page.next) {
    page.history.push(page.cursor);
    page.cursor = page.next;
  } else if (direction === "prev" && page.history.length) {
    page.cursor = page.history.pop();
  }
  const request = Symbol();
  page.request = request;
  const query = new URLSearchParams();
  if (page.cursor) query.set("cursor", page.cursor);
  const filter = kind === "matches" ? select("#match-phase").value : select("#player-presence").value;
  if (filter) query.set(kind === "matches" ? "phase" : "online", filter);
  const data = await api(`/${kind}?${query}`);
  if (request !== page.request) return;
  page.next = data.nextCursor;
  select(`#${kind}-prev`).disabled = page.history.length === 0;
  select(`#${kind}-next`).disabled = !page.next;
  if (kind === "matches") {
    rows("#match-rows", data.items.map((match) => [
      cellButton(match.id, () => openMatch(match.id)), match.queue, title(match.phase),
      `${match.humanPlayers} human · ${match.botPlayers ?? 0} bots`, date(match.createdAt),
    ]), 5, "No matches.");
  } else {
    rows("#player-rows", data.items.map((player) => [
      cellButton(player.displayName, () => openPlayer(player.id)), player.id,
      title(player.platformState), player.currentMatchId, date(player.lastSeenAt),
    ]), 5, "No players.");
  }
}

async function openPlayer(id) {
  const player = await api(`/players/${encodeURIComponent(id)}`);
  const detail = select("#player-detail");
  detail.textContent = `${player.displayName} · ${title(player.platformState)} · Last seen ${date(player.lastSeenAt)}`;
  detail.hidden = false;
}

function closeMatch() {
  clearTimeout(current.detailTimer);
  current.match = null;
  current.generation++;
  select("#detail").hidden = true;
}

async function openMatch(id) {
  closeMatch();
  current.match = id;
  current.eventAfter = 0;
  select("#timeline").replaceChildren();
  select("#termination").reset();
  await refreshMatch();
  select("#detail").scrollIntoView({block: "nearest"});
}

async function refreshMatch() {
  clearTimeout(current.detailTimer);
  if (!current.match) return;
  const generation = current.generation;
  const matchPath = `/matches/${encodeURIComponent(current.match)}`;
  const [match, events] = await Promise.all([
    api(matchPath), api(`${matchPath}/events?after=${current.eventAfter}`),
  ]);
  if (generation !== current.generation) return;
  if (match.endedAt && !select("#termination").hidden) {
    await Promise.all([overview(), navigate("matches")]);
  }
  select("#detail").hidden = false;
  select("#match-identity").textContent = `${match.id} · ${title(match.phase)}${match.endReason ? " · " + title(match.endReason) : ""}`;
  select("#termination").hidden = ["ending", "ended"].includes(match.phase);
  rows("#participants", match.participants.map((player) => [
    player.slot, player.displayName, player.hero, yes(player.locked), yes(player.controlConnected), yes(player.gameConnected),
  ]), 6, "No participants.");
  for (const event of events.items) {
    const item = document.createElement("li");
    item.textContent = `${date(event.at)} · ${event.message}`;
    select("#timeline").append(item);
  }
  current.eventAfter = events.nextAfter;
  if (events.hasMore || match.phase !== "ended") {
    current.detailTimer = setTimeout(() => refreshMatch().catch(fail), events.hasMore ? 0 : 1500);
  }
}

async function refresh() {
  await Promise.all([overview(), pools(), navigate("players"), navigate("matches"), refreshMatch()]);
}

select("#login").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.target;
  const submit = form.querySelector("button");
  submit.disabled = true;
  const body = JSON.stringify(Object.fromEntries(new FormData(form)));
  form.elements.password.value = "";
  try {
    await api("/session", {method: "POST", body});
    report();
    signedIn(true);
    await refresh();
  } catch (error) {
    fail(error);
  } finally {
    submit.disabled = false;
  }
});
select("#sign-out").addEventListener("click", async () => {
  try {
    await api("/session", {method: "DELETE"});
    report();
    signedIn(false);
  } catch (error) {
    fail(error);
  }
});
select("#refresh").addEventListener("click", () => refresh().catch(fail));
select("#match-phase").addEventListener("change", () => navigate("matches", "reset").catch(fail));
select("#player-presence").addEventListener("change", () => navigate("players", "reset").catch(fail));
select("#close-detail").addEventListener("click", closeMatch);
for (const kind of ["players", "matches"]) {
  for (const direction of ["prev", "next"]) {
    select(`#${kind}-${direction}`).addEventListener("click", () => navigate(kind, direction).catch(fail));
  }
}
select("#termination").addEventListener("submit", async (event) => {
  event.preventDefault();
  const field = event.target.elements.reason;
  if (!current.match || !field.value.trim()) return;
  const submit = event.target.querySelector("button");
  submit.disabled = true;
  const body = JSON.stringify({reason: field.value});
  try {
    await api(`/matches/${encodeURIComponent(current.match)}/termination`, {method: "PUT", body});
    field.value = "";
    await refreshMatch();
  } catch (error) {
    fail(error);
  } finally {
    submit.disabled = false;
  }
});

try {
  await api("/session");
  signedIn(true);
  await refresh();
} catch (error) {
  if (error.status === 401) signedIn(false);
  else fail(error);
}
