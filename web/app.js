import init, { Demo } from "./pkg/web_demo.js";

const $ = (s) => document.querySelector(s);
const SEED = 7;
const X0 = 6, X1 = 22;           // chart window, hours
const Y0 = -100, Y1 = 140;       // kW
const W = 760, H = 330, M = { l: 44, r: 16, t: 12, b: 28 };
const SAMPLE_S = 60;             // chart resolution, simulated seconds

// What each country's rules mean for this depot (mirrors control::policy and
// the example configurations).
const COUNTRY = {
  DE: {
    name: "Germany",
    dim: "§14a EnWG · C_SC_NA_1 · IOA 5001",
    floor: "Pmin,14a = 0.4 × 14 kW heat pump + 5 × 0.6 × 4.2 kW (4 chargers + battery) = <b class=\"mono\">18.2 kW</b> (BNetzA BK6-22-300). PV surplus and battery discharge may be used on top.",
    feed: "No standing cap for this site; the DSO sends setpoints.",
    dimTitle: "§14a dimming active",
  },
  AT: {
    name: "Austria",
    dim: "flexibility contract · C_SC_NA_1 · IOA 5001",
    floor: "No statutory minimum like §14a: this depot's contract keeps <b class=\"mono\">10 kW</b> and lets the DSO dim at most 2 hours a day.",
    feed: "ElWG Spitzenkappung: the DSO capped feed-in of this new PV system at <b>70%</b> of module peak power, always in force.",
    dimTitle: "Contract dimming active",
  },
  CH: {
    name: "Switzerland",
    dim: "flexibility contract · C_SC_NA_1 · IOA 5001",
    floor: "Contract: <b class=\"mono\">8 kW</b> minimum, at most 3 hours a day. The owner forbade the DSO to use the heat pump (StromVV Art. 19d), so it is never limited.",
    feed: "The DSO may curtail at most <b>3%</b> of the yearly PV energy for free (StromVV Art. 19c); beyond that only in an emergency. Late-year scenario: 3,400 of 3,420 kWh already used.",
    dimTitle: "Contract dimming active",
  },
};

let demo, history, lastSample, speed = 300, playing = true, lastFrameT = null, state, country = "DE", lastHour = 11.5;
const faults = new Set();

function start(hour, c = country) {
  country = c;
  lastHour = hour;
  demo = new Demo(hour, SEED, country);
  history = [];
  lastSample = -Infinity;
  faults.clear();
  document.querySelectorAll("#faults button").forEach((b) => b.setAttribute("aria-pressed", "false"));
  press("#dim-seg", "off");
  press("#feed-seg", "100");
  press("#emergency-seg", "off");
  press("#country-seg", country);
  const info = COUNTRY[country];
  $("#dim-sub").textContent = info.dim;
  $("#floor-text").innerHTML = info.floor;
  $("#country-note").innerHTML = info.feed;
  $("#log").replaceChildren();
  playing = true;
  $("#play").textContent = "❚❚";
  $("#play").setAttribute("aria-label", "Pause");
  tick(0);
}

function press(sel, v) {
  document.querySelectorAll(`${sel} button`).forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.v === v)));
}

function onSeg(sel, fn) {
  $(sel).addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (b) fn(b.dataset.v);
  });
}

function wire() {
  onSeg("#country-seg", (v) => start(lastHour, v));
  onSeg("#dim-seg", (v) => { demo.command_dim(v === "on"); press("#dim-seg", v); tick(0); });
  onSeg("#feed-seg", (v) => { demo.command_feed_in(Number(v)); press("#feed-seg", v); tick(0); });
  onSeg("#emergency-seg", (v) => { demo.command_emergency(v === "on"); press("#emergency-seg", v); tick(0); });
  onSeg("#speed-seg", (v) => { speed = Number(v); press("#speed-seg", v); });
  onSeg("#scene-seg", (v) => start(Number(v)));
  $("#play").addEventListener("click", () => {
    playing = !playing;
    $("#play").textContent = playing ? "❚❚" : "▶";
    $("#play").setAttribute("aria-label", playing ? "Pause" : "Play");
  });
  $("#faults").addEventListener("click", (e) => {
    const b = e.target.closest("button"); if (!b) return;
    const d = b.dataset.d, off = !faults.has(d);
    off ? faults.add(d) : faults.delete(d);
    demo.set_device_online(d, !off);
    b.setAttribute("aria-pressed", String(off));
  });
  $("#hex").addEventListener("change", () => {
    document.querySelectorAll("#log .hex").forEach((h) => (h.hidden = !$("#hex").checked));
  });
  const svg = $("#chart");
  svg.addEventListener("pointermove", hover);
  svg.addEventListener("pointerleave", () => { $("#tip").style.display = "none"; hoverT = null; drawChart(); });
}

function loop(ts) {
  const dt = lastFrameT == null ? 0 : Math.min(0.1, (ts - lastFrameT) / 1000);
  lastFrameT = ts;
  if (playing) {
    if (demo.time_s() >= X1 * 3600) { playing = false; $("#play").textContent = "▶"; }
    else tick(dt * speed);
  }
  requestAnimationFrame(loop);
}

function tick(simSeconds) {
  if (simSeconds > 0) demo.advance(simSeconds);
  state = JSON.parse(demo.state_json());
  if (state.t_s - lastSample >= SAMPLE_S || history.length === 0) {
    lastSample = state.t_s;
    history.push({
      t: state.t_s / 3600,
      grid: state.grid_kw ?? (state.base_kw + state.steuve_kw - state.pv_kw),
      pv: state.pv_kw,
      steuve: state.steuve_kw - Math.max(0, state.battery.kw),
      battery: state.battery.kw,
      budget: state.mode !== "normal" ? state.steuve_budget_kw : null,
      exportLimit: state.feed_in_in_force_pct < 100 ? -state.allowed_export_kw : null,
    });
  }
  appendFrames(JSON.parse(demo.take_frames_json()));
  render();
}

// ---------- rendering ----------

const hh = (s) => { const m = Math.floor(s / 60) % 1440; return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`; };
const kw = (v) => v == null ? "–" : `${v.toFixed(1)} kW`;
const x = (h) => M.l + (h - X0) / (X1 - X0) * (W - M.l - M.r);
const y = (v) => M.t + (Y1 - Math.max(Y0, Math.min(Y1, v))) / (Y1 - Y0) * (H - M.t - M.b);
const css = (v) => getComputedStyle(document.documentElement).getPropertyValue(v).trim();

function render() {
  $("#clock").textContent = hh(state.t_s);
  renderMode();
  renderGauge();
  renderBudget();
  renderStats();
  renderDevices();
  drawChart();
}

function renderMode() {
  const info = COUNTRY[state.jurisdiction];
  const m = {
    normal: ["✓", "var(--good)", "Normal operation", state.feed_in_in_force_pct < 100
      ? `Feed-in limit in force: ${state.feed_in_in_force_pct.toFixed(0)}%. Export held at ${state.allowed_export_kw.toFixed(0)} kW; the depot and the battery use the rest of the PV.`
      : "No DSO limit in force. Chargers and heat pump run at full power; the battery stores surplus solar and covers imports."],
    dimmed: ["↓", "var(--dso)", info.dimTitle, `Controllable devices may draw ${kw(state.steuve_budget_kw)} from the grid: the ${kw(state.floor_kw)} floor plus PV surplus, minus a 0.3 kW margin. The battery discharges on top.`],
    releasing: ["↗", "var(--warn)", "Gradual release", "The dimming ended. Power returns over 5 minutes so the feeder does not see a step."],
  }[state.mode];
  const notes = [];
  if (state.emergency) notes.push(`<span style="color:var(--critical)">Emergency: day limits, budgets and opt-outs do not apply.</span>`);
  for (const r of state.refusals) notes.push(`<span style="color:var(--warn)">Refused: ${r}.</span>`);
  if (state.fallbacks.length) notes.push(`<span style="color:var(--critical)">Fallback: ${state.fallbacks.join(", ")}</span>`);
  $("#mode").innerHTML = `<div class="icon" style="background:${m[1]}">${m[0]}</div><div><strong>${m[2]}</strong><span>${m[3]}</span>${notes.map((n) => `<br>${n}`).join("")}</div>`;
}

function renderGauge() {
  const max = 110, now = state.steuve_grid_kw;
  const lim = state.mode !== "normal" ? state.steuve_budget_kw : null;
  const g = $("#gauge");
  g.querySelector(".fill").style.width = `${Math.min(100, (now ?? 0) / max * 100)}%`;
  const l = g.querySelector(".lim");
  l.style.display = lim == null ? "none" : "block";
  if (lim != null) l.style.left = `${Math.min(99.5, lim / max * 100)}%`;
  $("#g-now").textContent = now == null ? "unknown (meter offline)" : `now ${kw(now)}`;
  $("#g-lim").textContent = lim == null ? "no limit" : `limit ${kw(lim)}`;
}

function renderBudget() {
  const box = $("#budget");
  if (state.budget_kwh == null) { box.hidden = true; return; }
  box.hidden = false;
  const used = Math.min(100, state.budget_used_pct ?? 0);
  box.querySelector(".fill").style.width = `${used}%`;
  box.querySelector(".fill").style.background = used >= 100 ? "var(--critical)" : "var(--s-pv)";
  $("#b-now").textContent = `${state.curtailed_kwh_year.toFixed(1)} kWh curtailed this year`;
  $("#b-lim").textContent = `budget ${state.budget_kwh.toFixed(0)} kWh`;
}

function renderStats() {
  const stats = [
    [kw(state.grid_kw), state.grid_kw == null ? "grid · meter offline" : state.grid_kw >= 0 ? "importing from grid" : "exporting to grid"],
    [kw(state.pv_kw), `PV · ${state.pv_available_kw.toFixed(0)} kW available · limit ${state.pv_limit_pct.toFixed(0)}%`],
    [`${state.dimmed_min_today.toFixed(0)} min`, "consumption dimmed today"],
    [`${state.outdoor_c.toFixed(1)} °C`, "outdoor temperature"],
  ];
  $("#stats").innerHTML = stats.map(([b, s]) => `<div class="stat"><b>${b}</b><span>${s}</span></div>`).join("");
}

function renderDevices() {
  const cards = state.chargers.map((c, i) => {
    const pct = c.needs_kwh ? Math.min(100, c.session_kwh / c.needs_kwh * 100) : 0;
    const car = c.needs_kwh ? `${c.session_kwh.toFixed(1)} / ${c.needs_kwh.toFixed(0)} kWh` : "no car";
    return `<div class="dev"><header><b>Charger ${i + 1}</b><span class="pill ${c.status}">${c.status}</span></header>
      <div class="big">${c.setpoint_a.toFixed(0)} A → ${c.current_a.toFixed(0)} A</div>
      <div class="sub">${c.kw.toFixed(1)} kW · ${car}</div><div class="soc"><i style="width:${pct}%"></i></div></div>`;
  });
  const hpStatus = !state.heat_pump_online ? "offline" : state.heat_pump_kw < state.heat_pump_demand_kw - 0.05 ? "limited" : "running";
  const optOut = state.heat_pump_opted_out ? " · opted out" : "";
  cards.push(`<div class="dev"><header><b>Heat pump</b><span class="pill ${hpStatus === "limited" ? "waiting" : hpStatus === "running" ? "charging" : "offline"}">${hpStatus}</span></header>
    <div class="big">${state.heat_pump_limit_kw.toFixed(1)} → ${state.heat_pump_kw.toFixed(1)} kW</div>
    <div class="sub">wants ${state.heat_pump_demand_kw.toFixed(1)} kW · 14 kW rated${optOut}</div></div>`);
  const b = state.battery;
  const bStatus = !b.online ? "offline" : b.watchdog ? "failsafe" : b.kw > 0.05 ? "charging" : b.kw < -0.05 ? "discharging" : "idle";
  cards.push(`<div class="dev"><header><b>Battery</b><span class="pill ${bStatus === "discharging" ? "waiting" : bStatus}">${bStatus}</span></header>
    <div class="big">${b.setpoint_kw.toFixed(1)} → ${b.kw.toFixed(1)} kW</div>
    <div class="sub">${b.soc_pct.toFixed(0)}% of 100 kWh · ±50 kW</div><div class="soc"><i style="width:${b.soc_pct}%"></i></div></div>`);
  $("#devices").innerHTML = cards.join("");
}

let hoverT = null;
function drawChart() {
  const svg = $("#chart");
  const parts = [];
  for (let v = -100; v <= 140; v += 20) {
    parts.push(`<line x1="${M.l}" x2="${W - M.r}" y1="${y(v)}" y2="${y(v)}" stroke="${css(v === 0 ? "--ink3" : "--hair")}" stroke-width="${v === 0 ? 1 : 0.6}"/>`);
    if (v % 40 === 0 || v === 0) parts.push(`<text x="${M.l - 6}" y="${y(v) + 4}" text-anchor="end">${v}</text>`);
  }
  for (let h = X0; h <= X1; h += 2) parts.push(`<text x="${x(h)}" y="${H - 8}" text-anchor="middle">${String(h).padStart(2, "0")}:00</text>`);
  // DSO command periods: a faint band with a label, so the dashed limit has context
  const spans = (key) => {
    const out = []; let cur = null;
    for (const p of history) {
      if (p[key] != null) { if (!cur) cur = { a: p.t, b: p.t }; cur.b = p.t; }
      else if (cur) { out.push(cur); cur = null; }
    }
    if (cur) out.push(cur);
    return out;
  };
  for (const [key, name] of [["budget", "dimming"], ["exportLimit", "feed-in limit"]]) {
    for (const s of spans(key)) {
      parts.push(`<rect x="${x(s.a)}" y="${M.t}" width="${Math.max(1, x(s.b) - x(s.a))}" height="${H - M.t - M.b}" fill="${css("--dso")}" opacity=".09"/>`);
      if (x(s.b) - x(s.a) > 40) parts.push(`<text class="lbl" x="${x(s.a) + 4}" y="${M.t + 12}" style="fill:${css("--dso")}">${name}</text>`);
    }
  }
  const series = [["grid", "--s-grid"], ["pv", "--s-pv"], ["steuve", "--s-steuve"], ["battery", "--s-bat"]];
  for (const [k, c] of series) {
    const d = history.map((p, i) => `${i ? "L" : "M"}${x(p.t).toFixed(1)},${y(p[k]).toFixed(1)}`).join("");
    parts.push(`<path d="${d}" fill="none" stroke="${css(c)}" stroke-width="2" stroke-linejoin="round"/>`);
  }
  for (const key of ["budget", "exportLimit"]) {
    let d = "", on = false;
    for (const p of history) {
      if (p[key] == null) { on = false; continue; }
      d += `${on ? "L" : "M"}${x(p.t).toFixed(1)},${y(p[key]).toFixed(1)}`; on = true;
    }
    if (d) parts.push(`<path d="${d}" fill="none" stroke="${css("--ink")}" stroke-width="1.5" stroke-dasharray="4 3"/>`);
  }
  const last = history[history.length - 1];
  if (last) {
    const labels = [["grid", "grid"], ["pv", "PV"], ["steuve", "loads"], ["battery", "battery"]]
      .map(([k, name]) => ({ name, y: y(last[k]) }))
      .sort((a, b) => a.y - b.y);
    for (let i = 1; i < labels.length; i++) labels[i].y = Math.max(labels[i].y, labels[i - 1].y + 14);
    for (const l of labels) {
      const lx = x(last.t) + 6;
      if (lx < W - 44) parts.push(`<text class="lbl" x="${lx}" y="${l.y + 4}">${l.name}</text>`);
    }
  }
  if (hoverT != null) {
    const p = nearest(hoverT);
    if (p) parts.push(`<line x1="${x(p.t)}" x2="${x(p.t)}" y1="${M.t}" y2="${H - M.b}" stroke="${css("--ink3")}" stroke-width="1"/>`);
  }
  svg.innerHTML = parts.join("");
}

function nearest(t) {
  let best = null;
  for (const p of history) if (!best || Math.abs(p.t - t) < Math.abs(best.t - t)) best = p;
  return best && Math.abs(best.t - t) < 0.25 ? best : null;
}

function hover(e) {
  const svg = $("#chart"), r = svg.getBoundingClientRect();
  const px = (e.clientX - r.left) / r.width * W;
  hoverT = X0 + (px - M.l) / (W - M.l - M.r) * (X1 - X0);
  const p = nearest(hoverT), tip = $("#tip");
  if (!p) { tip.style.display = "none"; drawChart(); return; }
  const lim = p.budget != null ? `<br>dimming budget <b>${kw(p.budget)}</b>` : "";
  const ex = p.exportLimit != null ? `<br>export limit <b>${kw(-p.exportLimit)}</b>` : "";
  tip.innerHTML = `${hh(p.t * 3600)}<br>grid <b>${kw(p.grid)}</b><br>PV <b>${kw(p.pv)}</b><br>chargers + heat pump <b>${kw(p.steuve)}</b><br>battery <b>${kw(p.battery)}</b>${lim}${ex}`;
  tip.style.display = "block";
  const box = svg.parentElement.getBoundingClientRect();
  let left = e.clientX - box.left + 14;
  if (left + tip.offsetWidth > box.width - 8) left = e.clientX - box.left - tip.offsetWidth - 14;
  tip.style.left = `${left}px`;
  tip.style.top = `${e.clientY - box.top - 20}px`;
  drawChart();
}

function appendFrames(frames) {
  if (!frames.length) return;
  const log = $("#log");
  log.querySelectorAll(".new").forEach((n) => n.classList.remove("new"));
  for (const f of frames) {
    const row = document.createElement("div");
    row.className = "new";
    const dir = f.dir === "dso" ? `<span class="d-dso">DSO → site</span>` : `<span class="d-site">site → DSO</span>`;
    row.innerHTML = `<span class="t">${hh(f.t_s)}</span>${dir}<span class="txt"></span>`;
    row.querySelector(".txt").textContent = f.text;
    const hex = document.createElement("span");
    hex.className = "hex";
    hex.textContent = f.hex;
    hex.hidden = !$("#hex").checked;
    row.appendChild(hex);
    log.prepend(row);
  }
  while (log.children.length > 120) log.lastChild.remove();
}

// URL scenario for reproducible views, e.g.
// ?country=CH&start=6&until=21&feed=0@11.5-13.5&dim=17.75-19&emergency=12.5&fault=battery0@18&pause
function scripted() {
  const q = new URLSearchParams(location.search);
  const c = (q.get("country") ?? "DE").toUpperCase();
  if (!q.has("start")) { if (COUNTRY[c]) country = c; return false; }
  start(Number(q.get("start")), COUNTRY[c] ? c : "DE");
  const until = Number(q.get("until") ?? q.get("start"));
  const span = (v) => v && v.split("-").map(Number);
  const dim = span(q.get("dim"));
  const [feedPct, feedSpan] = (q.get("feed") ?? "").split("@");
  const feed = span(feedSpan);
  const emergencyAt = q.has("emergency") ? Number(q.get("emergency")) : null;
  const faultAt = q.get("fault")?.split("@");
  let dimOn = false, feedOn = false, faulted = false, emergencyOn = false;
  while (demo.time_s() < until * 3600) {
    const h = demo.time_s() / 3600;
    if (dim && !dimOn && h >= dim[0] && h < dim[1]) { demo.command_dim(true); press("#dim-seg", "on"); dimOn = true; }
    if (dim && dimOn && h >= dim[1]) { demo.command_dim(false); press("#dim-seg", "off"); dim.length = 0; dimOn = false; }
    if (feed && !feedOn && h >= feed[0] && h < feed[1]) { demo.command_feed_in(Number(feedPct)); press("#feed-seg", feedPct); feedOn = true; }
    if (feed && feedOn && h >= feed[1]) { demo.command_feed_in(100); press("#feed-seg", "100"); feed.length = 0; feedOn = false; }
    if (emergencyAt != null && !emergencyOn && h >= emergencyAt) { demo.command_emergency(true); press("#emergency-seg", "on"); emergencyOn = true; }
    if (faultAt && !faulted && h >= Number(faultAt[1])) {
      demo.set_device_online(faultAt[0], false); faulted = true;
      document.querySelector(`#faults [data-d="${faultAt[0]}"]`)?.setAttribute("aria-pressed", "true");
      faults.add(faultAt[0]);
    }
    tick(30);
  }
  if (q.has("pause")) { playing = false; $("#play").textContent = "▶"; }
  return true;
}

await init();
wire();
if (!scripted()) start(11.5);
requestAnimationFrame(loop);
