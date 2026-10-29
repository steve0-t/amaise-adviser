"use strict";

(() => {
  const icons = {
    overview: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/>',
    activity: '<path d="M3 12h4l3-8 4 16 3-8h4"/>',
    book: '<path d="M12 5v16M3 4c4-1 6 0 9 2 3-2 5-3 9-2v15c-4-1-6 0-9 2-3-2-5-3-9-2V4Z"/>',
    shield: '<path d="m12 3 8 3v6c0 4-4 7-8 9-4-2-8-5-8-9V6l8-3Z"/><path d="m8 12 3 3 5-6"/>',
    upload: '<path d="M12 16V3m-5 5 5-5 5 5M4 15v5h16v-5"/>',
    arrow: '<path d="M4 12h16m-6-6 6 6-6 6"/>',
    calendar: '<rect x="4" y="5" width="16" height="16" rx="2"/><path d="M8 3v4m8-4v4M4 11h16m-10 4h4"/>',
    packet: '<path d="m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5"/>',
    turn: '<path d="M5 8h11a4 4 0 0 1 0 8H7m2-12L5 8l4 4m2 8-4-4 4-4"/>',
    search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
    result: '<path d="M7 3h10l3 3v15H4V3h3Zm9 0v5h4M8 12h8m-8 4h6"/>',
    question: '<path d="M8 8a4 4 0 1 1 6 3.5c-1.3.7-2 1.2-2 2.5m0 3h.01"/><circle cx="12" cy="12" r="10"/>',
    sent: '<path d="m3 11 18-8-8 18-3-8-7-2Zm7 2L21 3"/>',
    warning: '<path d="m12 3 10 18H2L12 3Zm0 6v5m0 3h.01"/>',
    spark: '<path d="m12 3 2.6 6.4L21 12l-6.4 2.6L12 21l-2.6-6.4L3 12l6.4-2.6L12 3Z"/>',
    copy: '<rect x="8" y="8" width="12" height="13" rx="2"/><path d="M16 8V3H3v13h5"/>',
    download: '<path d="M12 3v13m-5-5 5 5 5-5M4 16v5h16v-5"/>',
    check: '<path d="m5 12 4 4L19 6"/>',
    folder: '<path d="M3 6h7l2 3h9v12H3V6Z"/>',
  };
  const $ = (id) => document.getElementById(id);
  function node(tag, attrs, ...children) {
    const element = document.createElement(tag);
    for (const [key, value] of Object.entries(attrs || {})) {
      if (value === undefined || value === null || value === false) continue;
      if (key === "class") element.className = value;
      else if (key.startsWith("on")) element.addEventListener(key.slice(2).toLowerCase(), value);
      else if (key === "disabled") element.disabled = Boolean(value);
      else element.setAttribute(key, value === true ? "" : String(value));
    }
    for (const child of children.flat(Infinity)) {
      if (child !== null && child !== undefined && child !== false) element.append(child instanceof Node ? child : document.createTextNode(String(child)));
    }
    return element;
  }
  function icon(name, className = "") {
    const span = node("span", { class: `icon ${className}`, "aria-hidden": "true" });
    span.innerHTML = `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">${icons[name] || icons.packet}</svg>`;
    return span;
  }
  document.querySelectorAll("[data-icon]").forEach((element) => element.replaceWith(icon(element.dataset.icon)));

  let snapshot = { streams: [], events: [], advice: [], revision: 0, selected_stream_id: null, quality: {} };
  let policy = { commands: [], developers: [] };
  let selectedStream = null;
  let activeView = "overview";
  let activityFilter = "all";
  let activitySearch = "";
  let commandFilter = "all";
  let commandSearch = "";
  let visibleEvents = 50;
  let busy = false;
  let polling = false;
  let connected = false;
  let initialLoaded = false;
  let previousSignature = "";
  let toastTimer;
  let operationError = "";
  let connectionError = "";
  let streamGeneration = 0;

  async function api(path, options = {}) {
    const response = await fetch(path, { ...options, headers: { "Content-Type": "application/json", ...(options.headers || {}) } });
    let data;
    try { data = await response.json(); } catch { throw new Error(`The local service returned an unreadable response (${response.status}).`); }
    if (!response.ok) throw new Error(data.error?.message || `Request failed (${response.status}).`);
    return data;
  }
  function post(path, payload) { return api(path, { method: "POST", body: JSON.stringify(payload) }); }
  function toast(message) {
    clearTimeout(toastTimer);
    $("toast").textContent = message;
    $("toast").hidden = false;
    $("announcement").textContent = message;
    toastTimer = setTimeout(() => { $("toast").hidden = true; }, 5000);
  }
  function reportError(message) {
    operationError = message;
    updateError();
  }
  function updateError() {
    const message = operationError || connectionError;
    $("error-banner").hidden = !message;
    $("error-banner").replaceChildren(...(message ? [node("span", {}, message)] : []));
    if (operationError) $("error-banner").append(node("button", { type: "button", class: "error-dismiss", onClick: () => { operationError = ""; updateError(); }, "aria-label": "Dismiss error" }, "✕"));
  }
  function updateConnection() {
    const status = $("connection-status");
    status.classList.toggle("offline", !connected);
    status.replaceChildren(node("span", { class: "live-dot" }), connected ? "Local service connected" : "Local service unavailable");
  }
  function view(name) {
    activeView = name;
    document.querySelectorAll("[data-view]").forEach((button) => {
      button.classList.toggle("active", button.dataset.view === name);
      if (button.dataset.view === name) button.setAttribute("aria-current", "page"); else button.removeAttribute("aria-current");
    });
    $("breadcrumb-current").textContent = { overview: "Overview", activity: "Activity", commands: "Command guide" }[name];
    render();
  }
  async function refresh(force = false) {
    if (polling || busy) return;
    polling = true;
    const generation = streamGeneration;
    try {
      const query = selectedStream ? `?stream_id=${encodeURIComponent(selectedStream)}` : "";
      const next = await api(`/api/v2/state${query}`);
      if (busy || generation !== streamGeneration) return;
      connected = true;
      snapshot = next;
      if (next.policy) policy = next.policy;
      selectedStream = next.selected_stream_id;
      const signature = JSON.stringify(next);
      if (force || signature !== previousSignature || !initialLoaded) {
        previousSignature = signature;
        initialLoaded = true;
        updateStreams();
        render();
      }
      connectionError = "";
      updateError();
    } catch (error) {
      connected = false;
      connectionError = `Cannot reach the local adviser service. ${error.message} Start it with ./adviser/run.sh, then keep this page open.`;
      updateError();
      if (!initialLoaded) { initialLoaded = true; render(); }
    } finally {
      polling = false;
      updateConnection();
      if (generation !== streamGeneration && !busy) refresh(true);
    }
  }
  function updateStreams() {
    const select = $("stream-select");
    const streams = snapshot.streams || [];
    select.replaceChildren(...(streams.length ? streams.map((stream) => {
      const option = node("option", { value: stream.stream_id }, stream.stream_id);
      option.selected = stream.stream_id === selectedStream;
      return option;
    }) : [node("option", { value: "" }, "No stream yet")]));
    select.disabled = !streams.length;
    const current = streams.find((stream) => stream.stream_id === selectedStream);
    $("stream-description").textContent = current ? `Game ${current.game.game_id} · ${current.event_count} observed packets` : "Your client's updates will appear here.";
    $("nav-event-count").textContent = String(snapshot.events.length);
  }
  function render() {
    const focused = document.activeElement;
    const focusId = focused?.id;
    const cursor = typeof focused?.selectionStart === "number" ? [focused.selectionStart, focused.selectionEnd] : null;
    const content = activeView === "commands" ? commandPage() : projectPage();
    $("page-content").replaceChildren(content);
    if (focusId && $(focusId) && focused !== $(focusId)) {
      $(focusId).focus({ preventScroll: true });
      if (cursor && typeof $(focusId).setSelectionRange === "function") $(focusId).setSelectionRange(...cursor);
    }
  }
  function badge(text, type = "neutral") { return node("span", { class: `badge badge-${type}` }, text); }
  function button(text, handler, className = "button button-outline", iconName) {
    return node("button", { type: "button", class: className, onClick: handler }, iconName ? icon(iconName) : null, text);
  }
  function isDemo() { return snapshot.events.length > 0 && snapshot.events.every((event) => event.source === "demo"); }
  function stateLabel() {
    if (!snapshot.events.length) return badge("Awaiting observations", "neutral");
    if (isDemo()) return badge("Synthetic demo", "demo");
    if (snapshot.events.every((event) => event.source === "import")) return badge("Imported observations", "neutral");
    return badge("Observed client data", "green");
  }
  function projectPage() {
    const hasEvents = snapshot.events.length > 0;
    const header = node("div", { class: "page-heading" }, node("div", {},
      node("div", { class: "heading-eyebrow" }, node("span", { class: "eyebrow" }, "YOUR PROJECT, IN PERSPECTIVE"), stateLabel()),
      node("h1", {}, activeView === "activity" ? "Every update. A little clearer." : "A clearer view of your project."),
      node("p", { class: "page-description" }, "Understand the feedback. Consider the next step. Stay in control.")),
      hasEvents ? node("div", { class: "heading-actions" }, button("Export context", exportContext, "button button-outline", "download"), node("button", { type: "button", class: "button button-outline context-copy", onClick: copyContext, title: "Copy agent context as JSON", "aria-label": "Copy agent context as JSON" }, icon("copy"))) : null);
    const page = node("div", { class: "page-body" }, header);
    if (!hasEvents) {
      page.append(emptyWorkspace());
      return page;
    }
    page.append(metrics(), qualityNotice());
    if (isDemo()) page.append(node("div", { class: "demo-notice" }, icon("spark"), node("span", {}, node("strong", {}, "You're exploring a sample project. "), "All packets and advice in this stream are synthetic.")));
    page.append(node("div", { class: "project-grid" }, activityPanel(), advicePanel()));
    return page;
  }
  function emptyWorkspace() {
    const art = node("div", { class: "empty-art", "aria-hidden": "true" },
      node("div", { class: "art-orbit orbit-one" }), node("div", { class: "art-orbit orbit-two" }),
      node("div", { class: "art-card art-card-back" }, icon("activity")),
      node("div", { class: "art-card art-card-front" }, node("div", { class: "art-card-icon" }, icon("result")), node("span", { class: "art-line long" }), node("span", { class: "art-line" }), node("span", { class: "art-line short" }), node("span", { class: "art-check" }, icon("check"))));
    return node("div", { class: "empty-layout" }, node("section", { class: "empty-workspace panel" }, art,
      badge("A fresh perspective", "green"), node("h2", {}, "Your project story starts here."),
      node("p", {}, "Bring in AMEISE's JSON updates to turn old client feedback into a readable activity feed, with room for thoughtful advice."),
      node("div", { class: "empty-actions" }, button(busy ? "Loading sample…" : "Explore a sample project", loadDemo, "button button-primary", "spark"), button("Import your packets", openImport, "button button-outline", "upload")),
      node("p", { class: "empty-footnote" }, "The sample uses synthetic data. Your simulator is never controlled here.")),
      node("section", { class: "getting-started panel" }, node("div", { class: "eyebrow" }, "HOW IT FITS TOGETHER"),
        node("h2", {}, "An adviser beside your client."),
        ...[
          ["01", "Observe the project", "A client bridge sends updates to this local service. You can also import saved packets."],
          ["02", "Make sense of the feedback", "Read the timeline, inspect original packets, and share observed context with an agent."],
          ["03", "Choose your next move", "Review evidence-backed suggestions, then enter your chosen command in AMEISE yourself."],
        ].map(([number, title, text]) => node("div", { class: "start-step" }, node("span", { class: "step-number" }, number), node("div", {}, node("h3", {}, title), node("p", {}, text)))),
        node("div", { class: "agent-status-note" }, icon("spark"), node("span", {}, node("strong", {}, "No agent is running in this page."), " Connect an external agent to add advice; the timeline works on its own.")),
        node("a", { class: "text-link", href: "/docs/json-api-integration.md", target: "_blank", rel: "noopener" }, "Read the integration guide ", "↗")));
  }
  function metrics() {
    const events = snapshot.events;
    const latestDate = [...events].reverse().find((event) => event.payload.simulation_date_raw);
    const latestTurn = [...events].reverse().find((event) => event.turn_id !== null);
    const date = latestDate?.payload.simulation_date_raw || "Not reported";
    const cards = [
      ["calendar", "SIMULATION DATE", date, "As reported by the client"],
      ["turn", "LATEST OBSERVED TURN", latestTurn ? String(latestTurn.turn_id) : "Not reported", "Updates arrive as observations"],
      ["packet", "OBSERVED PACKETS", String(events.length), `Revision ${snapshot.revision} · ${snapshot.streams.length} ${snapshot.streams.length === 1 ? "stream" : "streams"}`],
    ];
    return node("div", { class: "metrics" }, ...cards.map(([symbol, label, value, detail]) => node("section", { class: "metric-card" }, node("div", { class: "metric-top" }, node("span", { class: "eyebrow" }, label), icon(symbol)), node("div", { class: `metric-value${value.length > 22 ? " metric-value-small" : ""}` }, value), node("p", {}, detail))));
  }
  function qualityNotice() {
    const quality = snapshot.quality || {};
    if (!quality.uncertain && !quality.missing_sequences?.length && !quality.truncated) return null;
    const parts = [];
    if (quality.missing_sequences?.length) parts.push(`Missing sequence numbers: ${quality.missing_sequences.slice(0, 12).join(", ")}${quality.missing_sequences.length > 12 ? ` (+${quality.missing_sequences.length - 12} more listed gaps)` : ""}.`);
    if (quality.uncertain) parts.push("A command may still be pending or its outcome is uncertain. Check the AMEISE client before deciding what to do.");
    if (quality.truncated) parts.push("The list of missing sequence numbers is truncated.");
    return node("div", { class: "quality-notice", role: "status" }, icon("warning"), node("div", {}, node("strong", {}, "The project picture is incomplete."), node("p", {}, parts.join(" "))));
  }
  function searchInput(id, placeholder, value, onChange) {
    return node("label", { class: "search-field" }, icon("search"), node("span", { class: "visually-hidden" }, placeholder), node("input", { id, type: "search", placeholder, value, onInput: (event) => onChange(event.target.value) }));
  }
  function activityPanel() {
    const allEvents = [...snapshot.events].reverse();
    const filtered = allEvents.filter((event) => {
      const matchKind = activityFilter === "all" || (activityFilter === "results" ? event.kind === "command.result" : event.kind === "callback.requested");
      return matchKind && (!activitySearch || JSON.stringify(event).toLowerCase().includes(activitySearch.toLowerCase()));
    });
    const tabs = node("div", { class: "filter-tabs", "aria-label": "Filter activity" }, ...[["all", "All updates"], ["results", "Feedback"], ["questions", "Questions"]].map(([value, label]) => node("button", { type: "button", class: activityFilter === value ? "selected" : "", "aria-pressed": activityFilter === value ? "true" : "false", onClick: () => { activityFilter = value; visibleEvents = 50; render(); } }, label)));
    const panel = node("section", { class: "activity-panel panel", "aria-labelledby": "activity-heading" },
      node("div", { class: "panel-heading" }, node("div", {}, node("div", { class: "eyebrow" }, "THE PROJECT JOURNAL"), node("h2", { id: "activity-heading" }, "Activity", node("span", { class: "count-pill" }, String(snapshot.events.length)))), node("span", { class: "quiet-label" }, "Newest first")),
      node("div", { class: "activity-tools" }, tabs, searchInput("activity-search", "Find an update", activitySearch, (value) => { activitySearch = value; visibleEvents = 50; render(); })));
    panel.append(node("div", { class: "timeline" }, ...(filtered.length ? filtered.slice(0, visibleEvents).map(eventCard) : [node("div", { class: "filtered-empty" }, icon("search"), node("h3", {}, "No updates match this view."), node("p", {}, "Try another filter or search term."))])));
    if (filtered.length > visibleEvents) panel.append(button(`Show ${Math.min(50, filtered.length - visibleEvents)} earlier updates`, () => { visibleEvents += 50; render(); }, "button button-quiet load-more"));
    panel.append(node("div", { class: "panel-footnote" }, icon("shield"), "Observed feedback only. A sent command does not confirm it took effect."));
    return panel;
  }
  function timeLabel(value) {
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }).format(date);
  }
  function eventCard(event) {
    const payload = event.payload;
    const kind = event.kind;
    const definitions = {
      "game.selected": ["folder", "Project selected", "neutral"],
      "command.sent": ["sent", "Command sent", "neutral"],
      "command.result": ["result", "Client feedback", "green"],
      "callback.requested": ["question", "Your input is needed", "amber"],
      "transport.uncertain": ["warning", "Outcome uncertain", "amber"],
    };
    let [symbol, title, tone] = definitions[kind] || ["packet", kind, "neutral"];
    const resolvedCallback = kind === "callback.requested" && snapshot.events.some((entry) => entry.kind === "command.result" && entry.payload.request_id === payload.request_id && entry.sequence > event.sequence);
    if (resolvedCallback) { title = "Client question · later result observed"; tone = "neutral"; }
    const body = node("div", { class: "event-body" }, node("div", { class: "event-heading" }, node("strong", {}, title), node("time", { datetime: event.occurred_at, title: event.occurred_at }, timeLabel(event.occurred_at))));
    if (payload.command) body.append(node("code", { class: "event-command" }, payload.command));
    if (kind === "game.selected") body.append(node("p", { class: "event-text" }, payload.name));
    if (kind === "command.sent") body.append(node("p", { class: "event-text text-muted" }, "Recorded as sent. Wait for client feedback to confirm the outcome."));
    if (kind === "command.result") body.append(node("p", { class: "event-text" }, payload.feedback || "The client returned no feedback text."));
    if (kind === "transport.uncertain") body.append(node("p", { class: "event-text" }, payload.message));
    if (kind === "callback.requested") {
      body.append(node("p", { class: "event-text" }, payload.question), node("div", { class: "callback-choices" }, ...payload.choices.map((choice) => node("span", {}, node("span", { class: "choice-number" }, String(choice.id)), choice.label))), node("p", { class: "callback-note" }, resolvedCallback ? "A later result was observed for this request. These choices are retained as history." : "Answer this question in the AMEISE client. The adviser cannot submit an answer."));
      if (!payload.choices.length) body.append(node("p", { class: "callback-note" }, "No structured answer options were supplied. Read the original client dialog."));
      if (payload.alternatives_raw) body.append(node("details", { class: "raw-details" }, node("summary", {}, "Original answer options"), node("pre", {}, payload.alternatives_raw)));
      if (payload.previous_answer_ids.length) body.append(node("p", { class: "callback-note" }, `Previous answer IDs: ${payload.previous_answer_ids.join(", ")}`));
    }
    const meta = node("div", { class: "event-meta" }, node("span", {}, `#${event.sequence}`), event.turn_id !== null ? node("span", {}, `Turn ${event.turn_id}`) : null, badge(event.source === "demo" ? "Synthetic" : event.source === "import" ? "Imported" : "Client", event.source === "demo" ? "demo" : "neutral"));
    const details = node("details", { class: "raw-details" }, node("summary", {}, "Inspect packet", node("span", { class: "mono-indicator" }, "{ }")), node("pre", {}, JSON.stringify(event, null, 2)));
    if (kind === "command.result" && payload.kernel_output_raw) details.append(node("div", { class: "raw-output-label" }, "RAW KERNEL OUTPUT"), node("pre", {}, payload.kernel_output_raw));
    body.append(meta, details);
    return node("article", { class: `event-card event-${tone}`, id: `packet-${event.event_id}`, tabindex: "-1" }, node("div", { class: `event-symbol symbol-${tone}` }, icon(symbol)), body);
  }
  function evidenceLinks(ids) {
    return node("div", { class: "evidence-links" }, "Based on ", ...ids.map((id) => {
      const event = snapshot.events.find((entry) => entry.event_id === id);
      return node("button", { type: "button", class: "evidence-link", title: id, onClick: () => {
        activityFilter = "all"; activitySearch = ""; visibleEvents = snapshot.events.length; view("activity");
        const card = $(`packet-${id}`);
        if (card) { card.scrollIntoView({ behavior: "smooth", block: "center" }); card.focus({ preventScroll: true }); card.classList.add("highlighted"); setTimeout(() => card.classList.remove("highlighted"), 2500); }
      } }, event ? `#${event.sequence}` : id);
    }));
  }
  function advicePanel() {
    const adviceRecords = [...(snapshot.advice || [])];
    const latest = adviceRecords.sort((a, b) => (b.based_on_revision - a.based_on_revision) || String(b.created_at || "").localeCompare(String(a.created_at || "")))[0];
    const stale = latest && (latest.stale || latest.based_on_revision !== snapshot.revision);
    const panel = node("aside", { class: "advice-panel panel", "aria-labelledby": "advice-heading" }, node("div", { class: "advice-header" }, node("div", { class: "advice-header-top" }, icon("spark"), badge(latest ? (latest.source === "demo" ? "Demo advice" : stale ? "Needs refresh" : "Agent advice") : "Awaiting agent advice", latest && !stale ? "sage" : "dark")), node("div", { class: "eyebrow" }, "A SECOND PERSPECTIVE"), node("h2", { id: "advice-heading" }, "Make the next step\na considered one.")));
    if (!latest) {
      panel.append(node("div", { class: "advice-empty" }, node("h3", {}, "Room for a little guidance."), node("p", {}, "No agent advice has been received for this stream. An external agent can read your project context and return observations with supporting evidence."), node("div", { class: "advice-waiting" }, node("span", { class: "hollow-dot" }), "Waiting for adviser notes"), button("Export agent context", exportContext, "button button-outline full-width", "download"), node("p", { class: "fine-print" }, "Exporting context downloads JSON. It does not start or contact an agent.")));
    } else {
      const body = node("div", { class: "advice-body" });
      if (stale) body.append(node("div", { class: "stale-notice" }, icon("warning"), node("span", {}, "New observations arrived after this advice. Ask your agent for a fresh review. Command copying is paused.")));
      body.append(node("p", { class: "advice-summary" }, latest.summary));
      if (latest.observations.length) body.append(node("div", { class: "eyebrow advice-section-label" }, "WHAT STANDS OUT"), ...latest.observations.map((observation) => node("div", { class: "observation" }, node("span", { class: "observation-dot" }), node("div", {}, node("p", {}, observation.text), evidenceLinks(observation.evidence_event_ids)))));
      if (latest.rendered_recommendations?.length) body.append(node("div", { class: "eyebrow advice-section-label" }, "POSSIBLE NEXT STEPS"), ...latest.rendered_recommendations.map((recommendation, index) => node("article", { class: "recommendation" }, node("div", { class: "recommendation-number" }, String(index + 1).padStart(2, "0")), node("p", {}, recommendation.reason), node("code", {}, recommendation.command), evidenceLinks(recommendation.evidence_event_ids), node("button", { type: "button", class: "button button-outline button-small copy-command", disabled: stale, onClick: () => copyText(recommendation.command, "Command copied. Enter it manually in AMEISE when you decide to proceed.") }, icon("copy"), stale ? "Awaiting fresh advice" : "Copy command"))));
      if (latest.missing_information.length) body.append(node("div", { class: "missing-info" }, node("h3", {}, "Before deciding"), node("ul", {}, ...latest.missing_information.map((text) => node("li", {}, text)))));
      body.append(node("div", { class: "advice-revision" }, `${latest.source === "demo" ? "Synthetic example" : "Agent review"} · Based on revision ${latest.based_on_revision}`, badge(stale ? "Stale" : "Current", stale ? "amber" : "green")), button("Export current context", exportContext, "button button-quiet full-width", "download"));
      panel.append(body);
    }
    panel.append(node("div", { class: "advice-footer" }, icon("shield"), "Suggestions respect the command policy. You choose and act in the original client."));
    return panel;
  }
  function commandPage() {
    const commands = policy.commands || [];
    const terms = commandSearch.trim().toLowerCase().split(/\s+/).filter(Boolean);
    const filtered = commands.filter((command) => {
      const searchText = [command.id, command.label, command.group, command.template, JSON.stringify(command.parameters)].join(" ").toLowerCase();
      return (commandFilter === "all" || command.recommendation === commandFilter) && terms.every((term) => searchText.includes(term));
    });
    const counts = { all: commands.length, allowed: 0, manual_only: 0, excluded: 0 };
    commands.forEach((command) => { counts[command.recommendation]++; });
    const filters = [["all", "All commands"], ["allowed", "Allowed advice"], ["manual_only", "Manual decision"], ["excluded", "Excluded from advice"]];
    const page = node("div", { class: "page-body command-page" }, node("div", { class: "page-heading" }, node("div", {}, node("div", { class: "heading-eyebrow" }, node("span", { class: "eyebrow" }, "THE COMMAND REFERENCE"), badge("QA 200 · v1.95", "green")), node("h1", {}, "Know your options."), node("p", { class: "page-description" }, "The documented commands, with clear boundaries for your adviser.")), node("a", { class: "button button-outline", href: "/docs/AMAISE_commandlist-qa200-en-v1.95.pdf", target: "_blank", rel: "noopener" }, icon("book"), "Original PDF ↗")),
      node("div", { class: "policy-explainer" }, ...[["green", "Allowed advice", "The agent may suggest these actions, with evidence."], ["amber", "Manual decision", "Reference only. These choices remain yours."], ["muted", "Excluded from advice", "Documented for completeness; withheld from suggestions."]].map(([tone, title, text]) => node("div", {}, badge(title, tone), node("p", {}, text)))),
      node("section", { class: "panel command-list-panel", "aria-label": "Documented command catalog" }, node("div", { class: "command-tools" }, node("div", { class: "filter-tabs command-filter-tabs", "aria-label": "Filter command policy" }, ...filters.map(([value, label]) => node("button", { type: "button", class: commandFilter === value ? "selected" : "", "aria-pressed": commandFilter === value ? "true" : "false", onClick: () => { commandFilter = value; render(); } }, label, node("span", {}, String(counts[value]))))), searchInput("command-search", "Search commands or parameters", commandSearch, (value) => { commandSearch = value; render(); })), node("div", { class: "command-results-label" }, `${filtered.length} documented ${filtered.length === 1 ? "form" : "forms"}`, node("span", {}, "Templates show parameter names in braces")),
        ...(filtered.length ? filtered.map(commandCard) : [node("div", { class: "filtered-empty" }, icon("search"), node("h3", {}, "No matching command forms."), node("p", {}, "Try a different term or policy filter."))]),
        node("div", { class: "panel-footnote" }, icon("book"), "Documented syntax is a reference. Availability still depends on the project state and client model.")));
    return page;
  }
  function commandCard(command) {
    const [label, tone] = { allowed: ["Allowed advice", "green"], manual_only: ["Manual decision", "amber"], excluded: ["Excluded", "muted"] }[command.recommendation];
    const params = Object.entries(command.parameters || {});
    const source = command.source || {};
    const sourcePath = `/docs/${String(source.file || "AMAISE_commandlist-qa200-en-v1.95.pdf").split("/").pop()}`;
    const card = node("article", { class: "command-card" }, node("div", { class: "command-card-heading" }, node("div", {}, node("span", { class: "command-group" }, command.group), node("h3", {}, command.label)), badge(label, tone)), node("code", { class: "command-template" }, command.template), node("p", { class: "command-reason" }, command.reason));
    if (params.length) card.append(node("details", { class: "parameter-details" }, node("summary", {}, `${params.length} ${params.length === 1 ? "parameter" : "parameters"}`), node("dl", {}, ...params.map(([name, details]) => [node("dt", {}, name), node("dd", {}, details.enum ? details.enum.join(" · ") : details.type === "integer" ? `${details.type}${details.minimum !== undefined ? ` · minimum ${details.minimum}` : ""}${details.maximum !== undefined ? ` · maximum ${details.maximum}` : ""}` : details.type)]))));
    card.append(node("a", { class: "command-source", href: `${sourcePath}${source.page ? `#page=${source.page}` : ""}`, target: "_blank", rel: "noopener" }, source.page ? `Source · command list, page ${source.page} ↗` : "Source · project documentation ↗"));
    return card;
  }
  async function copyText(text, message) {
    try {
      if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
      else {
        const textarea = node("textarea", { class: "copy-fallback", "aria-hidden": "true" }, text);
        document.body.append(textarea); textarea.select();
        const copied = document.execCommand("copy"); textarea.remove();
        if (!copied) throw new Error("Your browser blocked clipboard access.");
      }
      toast(message);
    } catch (error) { reportError(`Could not copy to clipboard. ${error.message}`); }
  }
  async function exportContext() {
    if (!selectedStream) { toast("Import observations or explore the sample first."); return; }
    try {
      const context = await api(`/api/v2/context?stream_id=${encodeURIComponent(selectedStream)}`);
      const url = URL.createObjectURL(new Blob([JSON.stringify(context, null, 2)], { type: "application/json" }));
      const link = node("a", { href: url, download: `ameise-context-${selectedStream.replace(/[^a-z0-9_-]/gi, "_")}.json` });
      document.body.append(link); link.click(); link.remove(); setTimeout(() => URL.revokeObjectURL(url), 1000);
      toast("Project context exported. Share this file with your external adviser agent.");
    } catch (error) { reportError(`Could not export project context. ${error.message}`); }
  }
  async function copyContext() {
    if (!selectedStream) { toast("Import observations or explore the sample first."); return; }
    try {
      const context = await api(`/api/v2/context?stream_id=${encodeURIComponent(selectedStream)}`);
      await copyText(JSON.stringify(context, null, 2), "Project context copied as JSON. Share it with your external adviser agent.");
    } catch (error) { reportError(`Could not copy project context. ${error.message}`); }
  }
  function openImport() {
    $("import-error").hidden = true;
    $("import-dialog").showModal();
    $("import-text").focus();
  }
  function closeImport() { if (!busy) $("import-dialog").close(); }
  function parsePackets(text) {
    const trimmed = text.trim();
    if (!trimmed) throw new Error("Paste at least one event or choose a JSON file.");
    let parsed;
    try { parsed = JSON.parse(trimmed); }
    catch {
      parsed = trimmed.split(/\r?\n/).filter((line) => line.trim()).map((line, index) => {
        try { return JSON.parse(line); } catch { throw new Error(`Line ${index + 1} is not valid JSON. JSONL requires one complete object per line.`); }
      });
    }
    const packets = Array.isArray(parsed) ? parsed : Array.isArray(parsed?.events) ? parsed.events : [parsed];
    if (!packets.length) throw new Error("The import contains no events.");
    if (packets.some((packet) => !packet || typeof packet !== "object" || Array.isArray(packet))) throw new Error("Each packet must be a JSON event object.");
    return packets;
  }
  async function importPackets(event) {
    event.preventDefault();
    if (busy) return;
    let packets;
    try {
      if (new Blob([$("import-text").value]).size > 1024 * 1024) throw new Error("The import is larger than 1 MB. Split it into smaller batches.");
      packets = parsePackets($("import-text").value);
    } catch (error) { $("import-error").textContent = error.message; $("import-error").hidden = false; return; }
    busy = true;
    streamGeneration++;
    $("submit-import").disabled = true;
    $("submit-import").textContent = "Importing…";
    $("import-error").hidden = true;
    let imported = 0;
    let duplicates = 0;
    try {
      for (const packet of packets) {
        const result = await post("/api/v2/events", packet);
        if (result.duplicate) duplicates++; else imported++;
        selectedStream = result.stream_id;
      }
      $("import-dialog").close();
      $("import-text").value = "";
      $("import-file").value = "";
      $("import-file-name").textContent = "Up to 1 MB";
      toast(`${imported} ${imported === 1 ? "observation" : "observations"} imported${duplicates ? `; ${duplicates} already present` : ""}.`);
    } catch (error) {
      $("import-error").textContent = `${imported ? `${imported} observations were accepted before this error. ` : ""}${duplicates ? `${duplicates} duplicate observations were skipped. ` : ""}Packet ${imported + duplicates + 1}: ${error.message}`;
      $("import-error").hidden = false;
    } finally {
      busy = false; $("submit-import").disabled = false;
      $("submit-import").replaceChildren("Import observations ", node("span", { "aria-hidden": "true" }, "→"));
      await refresh(true);
    }
  }
  async function loadDemo() {
    if (busy) return;
    busy = true; streamGeneration++; render();
    let accepted = 0;
    try {
      const example = await api("/api/v2/examples");
      const suffix = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
      const stream = `demo-${suffix}`;
      const eventIds = new Map(example.events.map((event, index) => [event.event_id, `demo-${suffix}-${index + 1}`]));
      const requests = new Map();
      const events = example.events.map((original) => {
        const event = structuredClone(original);
        event.stream_id = stream; event.event_id = eventIds.get(original.event_id); event.source = "demo";
        if (event.payload.request_id) {
          if (!requests.has(event.payload.request_id)) requests.set(event.payload.request_id, `request-${suffix}-${requests.size + 1}`);
          event.payload.request_id = requests.get(event.payload.request_id);
        }
        return event;
      });
      for (const event of events) { await post("/api/v2/events", event); accepted++; }
      selectedStream = stream;
      const advice = structuredClone(example.advice);
      advice.advice_id = `demo-advice-${suffix}`; advice.stream_id = stream; advice.source = "demo"; advice.based_on_revision = events.length;
      [...advice.observations, ...advice.recommendations].forEach((item) => { item.evidence_event_ids = item.evidence_event_ids.map((id) => eventIds.get(id) || id); });
      await post("/api/v2/advice", advice);
      activeView = "overview"; view("overview");
      toast("Sample project loaded. All events and advice in this stream are synthetic.");
    } catch (error) { reportError(`Could not finish loading the sample${accepted ? ` (${accepted} observations accepted)` : ""}. ${error.message}`); }
    finally { busy = false; await refresh(true); }
  }

  document.querySelectorAll("[data-view]").forEach((element) => element.addEventListener("click", () => view(element.dataset.view)));
  $("stream-select").addEventListener("change", async (event) => { streamGeneration++; selectedStream = event.target.value || null; visibleEvents = 50; await refresh(true); });
  $("open-import").addEventListener("click", openImport);
  $("close-import").addEventListener("click", closeImport);
  $("cancel-import").addEventListener("click", closeImport);
  $("import-form").addEventListener("submit", importPackets);
  $("import-dialog").addEventListener("cancel", (event) => { if (busy) event.preventDefault(); });
  $("import-dialog").addEventListener("click", (event) => { if (event.target === $("import-dialog")) closeImport(); });
  $("import-file").addEventListener("change", async (event) => {
    const file = event.target.files[0];
    if (!file) return;
    if (file.size > 1024 * 1024) { $("import-error").textContent = "Choose a file of 1 MB or less."; $("import-error").hidden = false; return; }
    try { $("import-text").value = await file.text(); $("import-file-name").textContent = file.name; $("import-error").hidden = true; }
    catch (error) { $("import-error").textContent = `Could not read this file. ${error.message}`; $("import-error").hidden = false; }
  });
  render();
  refresh(true);
  setInterval(() => { if (!document.hidden) refresh(); }, 2000);
  document.addEventListener("visibilitychange", () => { if (!document.hidden) refresh(true); });
})();
