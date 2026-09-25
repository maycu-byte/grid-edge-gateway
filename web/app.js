import init, { Demo, feeder_site, feeder_meta, day_summary_json, year_extremes_json } from "./pkg/web_demo.js";
import { LOCALE, UI, KEYS, T } from "./i18n.js";
import { BLOCKS, EXTRA } from "./blocks.js";
import { initYear, refreshYear } from "./year.js";

const $ = (s) => document.querySelector(s);
const SEED = 7;
const X0 = 6, X1 = 30;            // chart window, hours (06:00 to 06:00 next day)
const Y0 = -100, Y1 = 140;        // kW
const H = 330, M = { l: 44, r: 16, t: 12, b: 28 };
let W = 760;                      // drawing width; grows on wide screens so the charts keep their height
const CHART_PX = 380;             // on-screen height of the power chart, px
const SH = 110;                   // height of the small charts
const SAMPLE_S = 60;              // chart resolution, simulated seconds

let demo, history, lastSample, speed = 300, playing = true, lastFrameT = null, state, plan = null;
let country = "DE", season = "2025-01-20", strategy = "mpc", lastHour = 6;
const faults = new Set();

// ---------- language ----------

let lang = "en", L = T.en;
const textNodes = [];             // [node, English text, leading, trailing]
const blockEn = new Map();        // data-block element → English innerHTML
const keyEn = new Map();          // data-i18n element → English content

function pickLanguage() {
  const q = new URLSearchParams(location.search).get("lang");
  if (q && T[q]) return q;
  try { const v = localStorage.getItem("lang"); if (v && T[v]) return v; } catch { /* storage blocked */ }
  const nav = (navigator.language || "en").slice(0, 2).toLowerCase();
  return T[nav] ? nav : "en";
}

function collectText() {
  const known = (t) => t in UI.pt || t in EXTRA.pt;
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    if (n.parentElement.closest("script, style, [data-block], [data-i18n], [data-i18n-html]")) continue;
    const t = n.nodeValue.trim();
    if (!t || !known(t)) continue;
    const lead = n.nodeValue.match(/^\s*/)[0], trail = n.nodeValue.match(/\s*$/)[0];
    textNodes.push([n, t, lead, trail]);
  }
  document.querySelectorAll("[data-block]").forEach((e) => blockEn.set(e, e.innerHTML));
  document.querySelectorAll("[data-i18n]").forEach((e) => keyEn.set(e, e.textContent));
  document.querySelectorAll("[data-i18n-html]").forEach((e) => keyEn.set(e, e.innerHTML));
}

function applyLanguage(v) {
  lang = v; L = T[v];
  document.documentElement.lang = v;
  try { localStorage.setItem("lang", v); } catch { /* storage blocked */ }
  press("#lang-seg", v);
  for (const [n, en, lead, trail] of textNodes) {
    const tr = v === "en" ? en : UI[v][en] ?? EXTRA[v][en] ?? en;
    n.nodeValue = lead + tr + trail;
  }
  for (const [e, en] of blockEn) e.innerHTML = v === "en" ? en : BLOCKS[v][e.dataset.block] ?? en;
  document.querySelectorAll("[data-i18n]").forEach((e) => { e.textContent = KEYS[v][e.dataset.i18n] ?? keyEn.get(e); });
  document.querySelectorAll("[data-i18n-html]").forEach((e) => { e.innerHTML = KEYS[v][e.dataset.i18nHtml] ?? keyEn.get(e); });
  document.querySelectorAll("#log .d-dso").forEach((e) => { e.textContent = L.dsoToSite; });
  document.querySelectorAll("#log .d-site").forEach((e) => { e.textContent = L.siteToDso; });
  if (state) { countryTexts(); render(); fitCharts(); }
  syncCalc();
  if (calcRows.length) renderCompare();
  if (shown) { renderFeederKpis(shown.s, shown.m); drawAnim(); }
  refreshYear();
}

// Numbers in the reader's locale: 1.5 / 1,5 and 1,000 / 1.000.
const nf = (v, d = 0) => v.toLocaleString(LOCALE[lang], { minimumFractionDigits: d, maximumFractionDigits: d });

// ---------- live demo ----------

// Where the prices and the weather of a 2025 day come from, per country.
const ZONES = { DE: { bzn: "DE-LU", city: "stuttgart" }, AT: { bzn: "AT", city: "vienna" }, CH: { bzn: "CH", city: "zurich" } };
const EXTREME_DAYS = ["2025-01-20", "2025-05-11", "2025-11-22", "2025-07-01"];

function zoneTexts() {
  const z = ZONES[country];
  document.querySelectorAll(".z-bzn").forEach((e) => { e.textContent = z.bzn; });
  document.querySelectorAll(".z-city").forEach((e) => { e.textContent = L.cities[z.city]; });
  document.querySelectorAll(".z-country").forEach((e) => { e.textContent = L.countries[country]; });
  const day = (d) => new Date(`${d}T12:00:00`).toLocaleDateString(LOCALE[lang], { day: "numeric", month: "short" });
  const [dunkel, neg, cold, heat] = EXTREME_DAYS.map((d) => JSON.parse(day_summary_json(country, d)));
  $("#day-note").textContent = L.extremesNote(country, {
    dunkel: L.dayPeak(day(EXTREME_DAYS[0]), `${nf(dunkel.price_max)} €/MWh`, dunkel.price_max_h),
    neg: L.dayLow(day(EXTREME_DAYS[1]), `${nf(neg.price_min)} €/MWh`, neg.price_min_h),
    cold: L.dayCold(day(EXTREME_DAYS[2]), `${nf(cold.temp_mean, 1)} °C`),
    heat: L.dayHot(day(EXTREME_DAYS[3]), `${nf(heat.temp_max, 1)} °C`),
  });
  const y = JSON.parse(year_extremes_json(country));
  $("#price-key").textContent = L.priceKey(z.bzn, nf(y.max), `${day(y.max_date)}, ${y.max_h}:00`, nf(y.min), `${day(y.min_date)}, ${y.min_h}:00`);
}

function countryTexts() {
  zoneTexts();
  const info = L.country[country];
  $("#dim-sub").textContent = info.dim;
  $("#floor-text").innerHTML = info.floor;
  $("#country-note").innerHTML = info.feed;
  $("#strategy-note").textContent = L.strategy[strategy];
}

function start(hour, opts = {}) {
  country = opts.country ?? country;
  season = opts.season ?? season;
  strategy = opts.strategy ?? strategy;
  lastHour = hour;
  demo = new Demo(hour, SEED, country, season, strategy);
  history = [];
  lastSample = -Infinity;
  faults.clear();
  document.querySelectorAll("#faults button").forEach((b) => b.setAttribute("aria-pressed", "false"));
  press("#dim-seg", "off");
  press("#feed-seg", "100");
  press("#emergency-seg", "off");
  press("#country-seg", country);
  press("#season-seg", season);
  const button = document.querySelector(`#season-seg button[data-v="${season}"]`);
  $("#day-pick").classList.toggle("on", season.startsWith("2025") && !button);
  if (season.startsWith("2025")) $("#day-pick").value = season;
  press("#strategy-seg", strategy);
  countryTexts();
  $("#log").replaceChildren();
  setPlaying(true);
  tick(0);
  fitCharts();
}

// Play and pause; once the day is over, start it again.
function setPlaying(on) {
  playing = on;
  const over = demo && demo.time_s() >= X1 * 3600;
  $("#play").textContent = on ? "❚❚" : over ? "↺" : "▶";
  $("#play").setAttribute("aria-label", on ? "Pause" : over ? "Restart the day" : "Play");
  $("#play").title = over && !on ? L.restartDay : "";
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
  onSeg("#lang-seg", applyLanguage);
  onSeg("#country-seg", (v) => start(lastHour, { country: v }));
  onSeg("#season-seg", (v) => start(lastHour, { season: v }));
  $("#day-pick").addEventListener("change", (e) => { if (e.target.value) start(lastHour, { season: e.target.value }); });
  onSeg("#strategy-seg", (v) => {
    strategy = v;
    demo.set_strategy(v);
    press("#strategy-seg", v);
    $("#strategy-note").textContent = L.strategy[v];
    tick(0);
  });
  onSeg("#dim-seg", (v) => { demo.command_dim(v === "on"); press("#dim-seg", v); tick(0); });
  onSeg("#feed-seg", (v) => { demo.command_feed_in(Number(v)); press("#feed-seg", v); tick(0); });
  onSeg("#emergency-seg", (v) => { demo.command_emergency(v === "on"); press("#emergency-seg", v); tick(0); });
  onSeg("#speed-seg", (v) => { speed = Number(v); press("#speed-seg", v); });
  onSeg("#scene-seg", (v) => start(Number(v)));
  $("#play").addEventListener("click", () => {
    // at the end of the simulated day, play starts the day again
    if (demo.time_s() >= X1 * 3600) { start(X0); return; }
    setPlaying(!playing);
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
  for (const id of ["#chart", "#soc-chart", "#temp-chart", "#price-chart"]) {
    const svg = $(id);
    svg.addEventListener("pointermove", (e) => hover(e, svg));
    svg.addEventListener("pointerleave", () => { $("#tip").style.display = "none"; hoverT = null; drawCharts(); });
  }
}

function loop(ts) {
  const dt = lastFrameT == null ? 0 : Math.min(0.1, (ts - lastFrameT) / 1000);
  lastFrameT = ts;
  if (playing) {
    if (demo.time_s() >= X1 * 3600) setPlaying(false);
    else tick(dt * speed);
  }
  animTick(dt);
  requestAnimationFrame(loop);
}

function tick(simSeconds) {
  if (simSeconds > 0) demo.advance(simSeconds);
  state = JSON.parse(demo.state_json());
  plan = JSON.parse(demo.plan_json());
  if (state.t_s - lastSample >= SAMPLE_S || history.length === 0) {
    lastSample = state.t_s;
    history.push({
      t: state.t_s / 3600,
      grid: state.grid_kw ?? (state.base_kw + state.steuve_kw - state.pv_kw),
      pv: state.pv_kw,
      steuve: state.steuve_kw - Math.max(0, state.battery.kw),
      battery: state.battery.kw,
      soc: state.battery.soc_pct,
      indoor: state.indoor_c,
      tmin: state.comfort_min_c,
      budget: state.mode !== "normal" ? state.steuve_budget_kw : null,
      exportLimit: state.feed_in_in_force_pct < 100 ? -state.allowed_export_kw : null,
    });
  }
  appendFrames(JSON.parse(demo.take_frames_json()));
  render();
}

// ---------- rendering ----------

const hh = (s) => { const m = Math.floor(s / 60) % 1440; return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`; };
const kw = (v) => v == null ? "–" : `${nf(v, 1)} kW`;
const eur = (v) => `${nf(v, 2)} €`;
const x = (h) => M.l + (h - X0) / (X1 - X0) * (W - M.l - M.r);
const yk = (v, lo, hi, height) => M.t + (hi - Math.max(lo, Math.min(hi, v))) / (hi - lo) * (height - M.t - M.b);
const y = (v) => yk(v, Y0, Y1, H);
const css = (v) => getComputedStyle(document.documentElement).getPropertyValue(v).trim();

function render() {
  $("#clock").textContent = hh(state.t_s);
  renderMode();
  renderGauge();
  renderBudget();
  renderStats();
  renderVersus();
  renderDevices();
  drawCharts();
}

function renderMode() {
  const info = L.country[state.jurisdiction];
  const m = {
    normal: ["✓", "var(--good)", L.normal, state.feed_in_in_force_pct < 100
      ? L.feedInForce(nf(state.feed_in_in_force_pct), nf(state.allowed_export_kw))
      : state.strategy === "rules" ? L.noLimitRules : L.noLimitPlan],
    dimmed: ["↓", "var(--dso)", info.dimTitle, L.dimmedText(kw(state.steuve_budget_kw), kw(state.floor_kw))],
    releasing: ["↗", "var(--warn)", L.releasing, L.releasingText],
  }[state.mode];
  const notes = [];
  if (state.emergency) notes.push(`<span style="color:var(--critical)">${L.emergency}</span>`);
  for (const r of state.refusals) notes.push(`<span style="color:var(--warn)">${L.refused(r)}</span>`);
  if (state.fallbacks.length) notes.push(`<span style="color:var(--critical)">${L.fallback(state.fallbacks.join(", "))}</span>`);
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
  $("#g-now").textContent = now == null ? L.gUnknown : L.gNow(kw(now));
  $("#g-lim").textContent = lim == null ? L.gNoLimit : L.gLimit(kw(lim));
}

function renderBudget() {
  const box = $("#budget");
  if (state.budget_kwh == null) { box.hidden = true; return; }
  box.hidden = false;
  const used = Math.min(100, state.budget_used_pct ?? 0);
  box.querySelector(".fill").style.width = `${used}%`;
  box.querySelector(".fill").style.background = used >= 100 ? "var(--critical)" : "var(--s-pv)";
  $("#b-now").textContent = L.bCurtailed(nf(state.curtailed_kwh_year, 1));
  $("#b-lim").textContent = L.bBudget(nf(state.budget_kwh));
}

function renderStats() {
  const stats = [
    [kw(state.grid_kw), state.grid_kw == null ? L.sMeterOff : state.grid_kw >= 0 ? L.sImport : L.sExport],
    [`${nf(state.price_eur_mwh)} €/MWh`, L.sPrice(nf(state.import_eur_kwh * 100, 1))],
    [`${nf(state.indoor_c, 1)} °C`, L.sIndoor(nf(state.comfort_min_c), nf(state.outdoor_c, 1))],
    [kw(state.pv_kw), L.sPv(nf(state.pv_available_kw), nf(state.pv_limit_pct))],
  ];
  $("#stats").innerHTML = stats.map(([b, s]) => `<div class="stat"><b>${b}</b><span>${s}</span></div>`).join("");
}

function renderVersus() {
  const total = state.cost_eur + state.ageing_eur + state.demand_eur;
  const shadow = state.shadow_cost_eur + state.shadow_ageing_eur + state.shadow_demand_eur;
  const saved = shadow - total;
  const n = L.vRows;
  const rows = [
    [n[0], eur(state.cost_eur), eur(state.shadow_cost_eur)],
    [n[1], eur(state.ageing_eur), eur(state.shadow_ageing_eur)],
    [n[2], eur(state.demand_eur), eur(state.shadow_demand_eur)],
    [n[3], `${nf(state.peak_kw)} kW`, `${nf(state.shadow_peak_kw)} kW`],
    [n[4], `<b>${eur(total)}</b>`, `<b>${eur(shadow)}</b>`],
    [n[5], `${nf(state.discomfort_kh, 2)} K·h`, `${nf(state.shadow_discomfort_kh, 2)} K·h`],
    [n[6], `${nf(state.ev_unmet_kwh, 1)} kWh`, `${nf(state.shadow_ev_unmet_kwh, 1)} kWh`],
  ];
  const extraKwh = state.battery_kwh - state.shadow_battery_kwh;
  const warmer = state.indoor_c - state.shadow_indoor_c;
  const ahead = [
    extraKwh > 1 ? L.vBattery(nf(extraKwh)) : "",
    warmer > 0.3 ? L.vWarmer(nf(warmer, 1)) : "",
  ].filter(Boolean).join(L.vAnd);
  const head = state.strategy === "rules" ? L.vBoth : saved >= 0 ? L.vSaved(eur(saved)) : L.vBehind(eur(-saved), ahead);
  const solve = state.plans ? `<br><span class="muted">${L.vSolve(nf(state.plans), nf(state.solve_ms))}</span>` : "";
  $("#versus").innerHTML = `<p style="font-size:.9rem">${head}${solve}</p>
    <table class="vs"><thead><tr><th></th><th>${L.vHead[0]}</th><th>${L.vHead[1]}</th></tr></thead><tbody>
    ${rows.map((r) => `<tr><td>${r[0]}</td><td class="mono">${r[1]}</td><td class="mono">${r[2]}</td></tr>`).join("")}
    </tbody></table>`;
}

function renderDevices() {
  const st = (s) => L.st[s] ?? s;
  const cards = state.chargers.map((c, i) => {
    const pct = c.needs_kwh ? Math.min(100, c.session_kwh / c.needs_kwh * 100) : 0;
    const car = c.needs_kwh ? `${nf(c.session_kwh, 1)} / ${nf(c.needs_kwh)} kWh` : L.dNoCar;
    const leaves = c.leaves_in_h != null ? ` · ${L.dLeaves(c.leaves_in_h < 1 ? `${Math.round(c.leaves_in_h * 60)} min` : `${nf(c.leaves_in_h, 1)} h`)}` : "";
    return `<div class="dev"><header><b>${L.dCharger} ${i + 1}</b><span class="pill ${c.status}">${st(c.status)}</span></header>
      <div class="big">${nf(c.setpoint_a)} A → ${nf(c.current_a)} A</div>
      <div class="sub">${nf(c.kw, 1)} kW · ${car}${leaves}</div><div class="soc"><i style="width:${pct}%"></i></div></div>`;
  });
  const hpStatus = !state.heat_pump_online ? "offline" : state.heat_pump_external ? "planned" : state.heat_pump_kw < state.heat_pump_demand_kw - 0.05 ? "limited" : "thermostat";
  const optOut = state.heat_pump_opted_out ? L.dOptOut : "";
  cards.push(`<div class="dev"><header><b>${L.dHp}</b><span class="pill ${hpStatus === "limited" ? "waiting" : hpStatus === "offline" ? "offline" : "charging"}">${st(hpStatus)}</span></header>
    <div class="big">${nf(state.heat_pump_limit_kw, 1)} → ${nf(state.heat_pump_kw, 1)} kW</div>
    <div class="sub">${L.dInside(nf(state.indoor_c, 1))}${optOut}</div></div>`);
  const b = state.battery;
  const bStatus = !b.online ? "offline" : b.watchdog ? "failsafe" : b.kw > 0.05 ? "charging" : b.kw < -0.05 ? "discharging" : "idle";
  cards.push(`<div class="dev"><header><b>${L.dBattery}</b><span class="pill ${bStatus === "discharging" ? "waiting" : bStatus}">${st(bStatus)}</span></header>
    <div class="big">${nf(b.setpoint_kw, 1)} → ${nf(b.kw, 1)} kW</div>
    <div class="sub">${L.dSoc(nf(b.soc_pct))}</div><div class="soc"><i style="width:${b.soc_pct}%"></i></div></div>`);
  $("#devices").innerHTML = cards.join("");
}

// ---------- charts ----------

let hoverT = null;

function axisX(parts, height) {
  for (let h = X0; h <= X1; h += 3) {
    parts.push(`<text x="${x(h)}" y="${height - 8}" text-anchor="middle">${String(h % 24).padStart(2, "0")}:00</text>`);
  }
}

function spans(key) {
  const out = []; let cur = null;
  for (const p of history) {
    if (p[key] != null) { if (!cur) cur = { a: p.t, b: p.t }; cur.b = p.t; }
    else if (cur) { out.push(cur); cur = null; }
  }
  if (cur) out.push(cur);
  return out;
}

function bands(parts, height, labels) {
  for (const [key, name] of [["budget", L.cDimming], ["exportLimit", L.cFeedin]]) {
    for (const s of spans(key)) {
      parts.push(`<rect x="${x(s.a)}" y="${M.t}" width="${Math.max(1, x(s.b) - x(s.a))}" height="${height - M.t - M.b}" fill="${css("--dso")}" opacity=".09"/>`);
      if (labels && x(s.b) - x(s.a) > 40) parts.push(`<text class="lbl" x="${x(s.a) + 4}" y="${M.t + 12}" style="fill:${css("--dso")}">${name}</text>`);
    }
  }
}

function line(parts, pts, color, dash = "") {
  const d = pts.map((p, i) => `${i ? "L" : "M"}${p[0].toFixed(1)},${p[1].toFixed(1)}`).join("");
  if (d) parts.push(`<path d="${d}" fill="none" stroke="${color}" stroke-width="2" stroke-linejoin="round"${dash ? ` stroke-dasharray="${dash}"` : ""}/>`);
}

function crosshair(parts, height) {
  if (hoverT == null) return;
  const p = nearest(hoverT);
  if (p) parts.push(`<line x1="${x(p.t)}" x2="${x(p.t)}" y1="${M.t}" y2="${height - M.b}" stroke="${css("--ink3")}" stroke-width="1"/>`);
}

function planPoints(values, offset = 0) {
  if (!plan) return [];
  const pts = [];
  values.forEach((v, k) => {
    const t = plan.start_h + (k + offset) * plan.step_h;
    if (t >= X0 && t <= X1) pts.push([t, v]);
  });
  return pts;
}

// On a wide screen the drawings get wider instead of taller. Next to the
// side column, the four charts together grow to end level with it. Run on
// load, scene, language and resize only: the side column's text changes
// every frame, and following it would make the charts jump between sizes.
const SVG_HEIGHT = H + 3 * SH;    // the four charts' drawing heights together
function fitCharts() {
  const charts = $("#charts"), side = $(".main .side"), chart = $("#chart");
  const cw = chart.clientWidth;
  if (!cw) return;
  let w = Math.round(cw * H / CHART_PX);
  if (side.offsetTop === charts.offsetTop) {
    // everything in the panel that is not a chart keeps its height
    const fixed = charts.offsetHeight - SVG_HEIGHT * cw / W;
    const room = side.offsetHeight - fixed;
    if (room > 0) w = Math.round(SVG_HEIGHT * cw / room);
    w = Math.min(Math.round(cw * H / CHART_PX), Math.max(Math.round(cw * H / 640), w));
  }
  W = Math.max(480, w);
  $("#chart").setAttribute("viewBox", `0 0 ${W} ${H}`);
  for (const id of ["#price-chart", "#soc-chart", "#temp-chart"]) $(id).setAttribute("viewBox", `0 0 ${W} ${SH}`);
  if (state) drawCharts();
}

function drawCharts() {
  drawPower();
  drawPrice();
  drawSoc();
  drawTemp();
}

function drawPower() {
  const parts = [];
  for (let v = -100; v <= 140; v += 20) {
    parts.push(`<line x1="${M.l}" x2="${W - M.r}" y1="${y(v)}" y2="${y(v)}" stroke="${css(v === 0 ? "--ink3" : "--hair")}" stroke-width="${v === 0 ? 1 : 0.6}"/>`);
    if (v % 40 === 0 || v === 0) parts.push(`<text x="${M.l - 6}" y="${y(v) + 4}" text-anchor="end">${v}</text>`);
  }
  axisX(parts, H);
  bands(parts, H, true);
  for (const [k, c] of [["grid", "--s-grid"], ["pv", "--s-pv"], ["steuve", "--s-steuve"], ["battery", "--s-bat"]]) {
    line(parts, history.map((p) => [x(p.t), y(p[k])]), css(c));
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
    const labels = [["grid", L.cGrid], ["pv", L.cPv], ["steuve", L.cLoads], ["battery", L.cBattery]]
      .map(([k, name]) => ({ name, y: y(last[k]) }))
      .sort((a, b) => a.y - b.y);
    for (let i = 1; i < labels.length; i++) labels[i].y = Math.max(labels[i].y, labels[i - 1].y + 14);
    for (const l of labels) {
      const lx = x(last.t) + 6;
      if (lx < W - 60) parts.push(`<text class="lbl" x="${lx}" y="${l.y + 4}">${l.name}</text>`);
    }
  }
  crosshair(parts, H);
  $("#chart").innerHTML = parts.join("");
}

function drawPrice() {
  const lo = -300, hi = 600, yp = (v) => yk(v, lo, hi, SH);
  const parts = [];
  for (const v of [-300, 0, 300, 600]) {
    parts.push(`<line x1="${M.l}" x2="${W - M.r}" y1="${yp(v)}" y2="${yp(v)}" stroke="${css(v === 0 ? "--ink3" : "--hair")}" stroke-width="${v === 0 ? 1 : 0.6}"/>`);
    parts.push(`<text x="${M.l - 6}" y="${yp(v) + 4}" text-anchor="end">${v}</text>`);
  }
  axisX(parts, SH);
  bands(parts, SH, false);
  const prices = JSON.parse(demo.prices_json(X0, X1));
  let d = "";
  prices.forEach(([h, p], i) => { d += `${i ? "L" : "M"}${x(h).toFixed(1)},${yp(p).toFixed(1)}H${x(h + 1).toFixed(1)}`; });
  parts.push(`<path d="${d}" fill="none" stroke="${css("--ink2")}" stroke-width="1.5"/>`);
  parts.push(`<line x1="${x(state.t_s / 3600)}" x2="${x(state.t_s / 3600)}" y1="${M.t}" y2="${SH - M.b}" stroke="${css("--ink3")}" stroke-width="1"/>`);
  crosshair(parts, SH);
  $("#price-chart").innerHTML = parts.join("");
}

function drawSoc() {
  const ys = (v) => yk(v, 0, 100, SH);
  const parts = [];
  for (const v of [0, 50, 100]) {
    parts.push(`<line x1="${M.l}" x2="${W - M.r}" y1="${ys(v)}" y2="${ys(v)}" stroke="${css("--hair")}" stroke-width="0.6"/>`);
    parts.push(`<text x="${M.l - 6}" y="${ys(v) + 4}" text-anchor="end">${v}</text>`);
  }
  axisX(parts, SH);
  bands(parts, SH, false);
  line(parts, planPoints(plan ? plan.soc_pct : []).map(([t, v]) => [x(t), ys(v)]), css("--s-bat"), "4 3");
  line(parts, history.map((p) => [x(p.t), ys(p.soc)]), css("--s-bat"));
  crosshair(parts, SH);
  $("#soc-chart").innerHTML = parts.join("");
}

function drawTemp() {
  const lo = 16, hi = 24, yt = (v) => yk(v, lo, hi, SH);
  const parts = [];
  for (const v of [16, 20, 24]) {
    parts.push(`<line x1="${M.l}" x2="${W - M.r}" y1="${yt(v)}" y2="${yt(v)}" stroke="${css("--hair")}" stroke-width="0.6"/>`);
    parts.push(`<text x="${M.l - 6}" y="${yt(v) + 4}" text-anchor="end">${v}</text>`);
  }
  axisX(parts, SH);
  bands(parts, SH, false);
  // comfort floor: 20 °C from 06:00 to 22:00, 17 °C at night
  let d = "";
  for (let h = X0; h < X1; h += 0.25) {
    const floor = (h % 24) >= 6 && (h % 24) < 22 ? 20 : 17;
    d += `${h === X0 ? "M" : "L"}${x(h).toFixed(1)},${yt(floor).toFixed(1)}`;
  }
  parts.push(`<path d="${d}" fill="none" stroke="${css("--critical")}" stroke-width="1" stroke-dasharray="2 3"/>`);
  line(parts, planPoints(plan ? plan.indoor_c : []).map(([t, v]) => [x(t), yt(v)]), css("--s-steuve"), "4 3");
  line(parts, history.map((p) => [x(p.t), yt(p.indoor)]), css("--s-steuve"));
  crosshair(parts, SH);
  $("#temp-chart").innerHTML = parts.join("");
}

function nearest(t) {
  let best = null;
  for (const p of history) if (!best || Math.abs(p.t - t) < Math.abs(best.t - t)) best = p;
  return best && Math.abs(best.t - t) < 0.25 ? best : null;
}

function hover(e, svg) {
  const r = svg.getBoundingClientRect();
  const px = (e.clientX - r.left) / r.width * W;
  hoverT = X0 + (px - M.l) / (W - M.l - M.r) * (X1 - X0);
  const p = nearest(hoverT), tip = $("#tip");
  if (!p) { tip.style.display = "none"; drawCharts(); return; }
  const lim = p.budget != null ? `<br>${L.tBudget} <b>${kw(p.budget)}</b>` : "";
  const ex = p.exportLimit != null ? `<br>${L.tExport} <b>${kw(-p.exportLimit)}</b>` : "";
  tip.innerHTML = `${hh(p.t * 3600)}<br>${L.tGrid} <b>${kw(p.grid)}</b><br>${L.tPv} <b>${kw(p.pv)}</b><br>${L.tLoads} <b>${kw(p.steuve)}</b><br>${L.tBattery} <b>${kw(p.battery)}</b> · ${nf(p.soc)}%<br>${L.tIndoor} <b>${nf(p.indoor, 1)} °C</b>${lim}${ex}`;
  tip.style.display = "block";
  const box = $("#charts").getBoundingClientRect();
  let left = e.clientX - box.left + 14;
  if (left + tip.offsetWidth > box.width - 8) left = e.clientX - box.left - tip.offsetWidth - 14;
  tip.style.left = `${left}px`;
  tip.style.top = `${e.clientY - box.top - 20}px`;
  drawCharts();
}

function appendFrames(frames) {
  if (!frames.length) return;
  const log = $("#log");
  log.querySelectorAll(".new").forEach((n) => n.classList.remove("new"));
  for (const f of frames) {
    const row = document.createElement("div");
    row.className = "new";
    const dir = f.dir === "dso" ? `<span class="d-dso">${L.dsoToSite}</span>` : `<span class="d-site">${L.siteToDso}</span>`;
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
// ?country=DE&season=winter&strategy=mpc&start=6&until=30&dim=17.5-19.5&pause
function scripted() {
  const q = new URLSearchParams(location.search);
  const c = (q.get("country") ?? country).toUpperCase();
  const opts = {
    country: T.en.country[c] ? c : "DE",
    season: q.get("season") ?? season,
    strategy: q.get("strategy") ?? strategy,
  };
  if (!q.has("start")) { country = opts.country; season = opts.season; strategy = opts.strategy; return false; }
  start(Number(q.get("start")), opts);
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
    tick(60);
  }
  if (q.has("pause")) setPlaying(false);
  return true;
}


// ---------- feeder calculator ----------
// Every site is simulated by the gateway's own code (closedloop::feeder via
// feeder_site); the page only sums the sites, computes the metrics and plays
// the evening back minute by minute.

const RELEASE = {
  step: { ramp: 0, delay: 0 },
  ramp: { ramp: 300, delay: 0 },
  wait10: { ramp: 300, delay: 600 },
  wait30: { ramp: 300, delay: 1800 },
  ramp30: { ramp: 1800, delay: 0 },
};
const REDUCTION_FROM = 17.5;
const baselines = new Map();
let calcRows = [], calcKey = "", calcBusy = false, shown = null;
let FEEDER_META;

const pressedIn = (sel) => document.querySelector(`${sel} button[aria-pressed="true"]`).dataset.v;
const optionKey = (s) => [s.ctl, s.release, s.groups].join("|");

function optionLabel(s) {
  const parts = [L.ctl[s.ctl], L.rel[s.release]];
  if (s.groups > 1) parts.push(L.groups(s.groups));
  return parts.join(" · ");
}

function calcSettings() {
  return {
    sites: Number($("#c-sites").value),
    cap: Number($("#c-cap").value),
    fleet: pressedIn("#c-fleet"),
    season: $("#c-day-pick").classList.contains("on") ? $("#c-day-pick").value : pressedIn("#c-season"),
    dur: Number(pressedIn("#c-dur")),
    release: pressedIn("#c-release"),
    groups: Number(pressedIn("#c-groups")),
    ctl: pressedIn("#c-ctl"),
  };
}

function syncCalc() {
  const s = calcSettings();
  $("#c-sites-v").textContent = s.sites;
  $("#c-cap-v").textContent = `${s.cap} kW`;
  $("#c-cap-sub").textContent = L.capSub(nf(s.cap * s.sites), s.sites);
}

const yieldToPage = () => new Promise((r) => setTimeout(r, 0));

// Each site's one-minute load, their sum and the customer-side totals.
async function simulateFeeder(s, withReduction, label, progress) {
  const rel = RELEASE[s.release];
  const total = { load: null, sites: [], ev: 0, discomfort: 0, cost: 0 };
  for (let i = 0; i < s.sites; i++) {
    progress(L.siteOf(label(), i + 1, s.sites));
    await yieldToPage();
    const r = JSON.parse(feeder_site(
      s.season, s.ctl, rel.ramp, rel.delay,
      withReduction ? REDUCTION_FROM : NaN, REDUCTION_FROM + s.dur,
      withReduction ? s.groups : 1, s.fleet === "mixed", 1001 + i, i,
    ));
    total.sites.push(r.load_kw);
    total.load = total.load ? total.load.map((v, k) => v + r.load_kw[k]) : r.load_kw.slice();
    total.ev += r.ev_unmet_kwh;
    total.discomfort += r.discomfort_kh;
    total.cost += r.energy_cost_eur;
  }
  return total;
}

function feederMetrics(s, run, base) {
  const meta = FEEDER_META;
  const idx = (h) => Math.min(run.load.length, Math.round((h - meta.from_h) * 3600 / meta.sample_s));
  const end = REDUCTION_FROM + s.dur;
  const cap = s.cap * s.sites;
  const after = run.load.slice(idx(end));
  const diff = after.map((v, k) => v - base.load[idx(end) + k]);
  const over = (load) => load.filter((v) => v > cap).length;
  const rises = run.load.slice(Math.max(0, idx(end) - 1)).map((v, k, a) => (k ? v - a[k - 1] : 0));
  return {
    cap,
    peakBefore: Math.max(...run.load.slice(idx(REDUCTION_FROM - 0.5), idx(REDUCTION_FROM))),
    peakAfter: Math.max(...after),
    rebound: Math.max(0, ...diff),
    pushed: diff.reduce((a, d) => a + Math.max(0, d), 0) * meta.sample_s / 3600,
    rise: Math.max(0, ...rises),
    minutesOver: over(run.load),
    minutesOverAfter: over(after),
    baseMinutesOver: over(base.load),
    ev: run.ev,
    evPerSite: run.ev / s.sites,
    discomfort: run.discomfort,
    cost: run.cost,
  };
}

async function calculate(list) {
  if (calcBusy) return;
  calcBusy = true;
  $("#c-run").disabled = $("#c-all").disabled = true;
  const progress = (t) => { $("#c-progress").textContent = t; $("#c-progress").dataset.state = "busy"; };
  try {
    const first = list[0];
    const key = [first.sites, first.cap, first.fleet, first.season, first.dur].join("|");
    if (key !== calcKey) { calcRows = []; calcKey = key; }
    let last = null;
    for (const [n, s] of list.entries()) {
      const tag = list.length > 1 ? ` (${n + 1}/${list.length})` : "";
      const bkey = [s.sites, s.fleet, s.season, s.ctl].join("|");
      if (!baselines.has(bkey)) baselines.set(bkey, await simulateFeeder(s, false, () => `${L.noRed(L.ctl[s.ctl])}${tag}`, progress));
      const base = baselines.get(bkey);
      const run = await simulateFeeder(s, true, () => `${optionLabel(s)}${tag}`, progress);
      const m = feederMetrics(s, run, base);
      calcRows = calcRows.filter((r) => optionKey(r.s) !== optionKey(s));
      last = { s, m, run, base };
      calcRows.push(last);
      renderCompare();
    }
    showOption(last);
    $("#c-progress").textContent = L.done(list.length, list.length * list[0].sites);
    $("#c-progress").dataset.state = "done";
  } finally {
    calcBusy = false;
    $("#c-run").disabled = $("#c-all").disabled = false;
  }
}

// ----- the evening, minute by minute -----

const anim = { k: 0, pos: 0, playing: false, speed: 15 };

function showOption(r) {
  shown = r;
  $("#c-empty").hidden = true;
  $("#c-out").hidden = false;
  $("#c-scrub").max = String(r.run.load.length - 1);
  $("#c-grid").innerHTML = "<i></i>".repeat(r.s.sites);
  renderFeederKpis(r.s, r.m);
  renderCompare();
  anim.pos = 0; anim.k = 0;
  setAnimPlaying(true);
  drawAnim();
}

function setAnimPlaying(on) {
  anim.playing = on;
  const over = shown && anim.pos >= shown.run.load.length - 1;
  $("#c-play").textContent = on ? "❚❚" : over ? "↺" : "▶";
  $("#c-play").setAttribute("aria-label", on ? "Pause" : over ? "Restart the evening" : "Play");
}

function animTick(dt) {
  if (!shown || !anim.playing) return;
  const n = shown.run.load.length;
  anim.pos = Math.min(n - 1, anim.pos + dt * anim.speed * 60 / FEEDER_META.sample_s);
  const k = Math.floor(anim.pos);
  if (k !== anim.k) { anim.k = k; drawAnim(); }
  if (anim.pos >= n - 1) setAnimPlaying(false);
}

// What the grid operator's timeline says at minute k.
function phaseAt(s, t) {
  const end = REDUCTION_FROM + s.dur;
  const lastRelease = end + (s.groups - 1) * 0.25 + (RELEASE[s.release].ramp + RELEASE[s.release].delay) / 3600;
  if (t < REDUCTION_FROM) return ["", L.aBefore];
  if (t < end) return ["dim", L.aDim];
  if (t < lastRelease) return ["rel", L.aRel];
  return ["", L.aAfter];
}

function drawAnim() {
  const { s, run, base, m } = shown, k = anim.k;
  const meta = FEEDER_META, dt = meta.sample_s / 3600;
  const t = meta.from_h + (k + 0.5) * dt;
  drawFeeder(s, run, base, m, k);
  $("#c-clock").textContent = hh(t * 3600);
  $("#c-scrub").value = String(k);
  const load = run.load[k];
  const [cls, text] = load > m.cap ? ["over", L.aOver] : phaseAt(s, t);
  const st = $("#c-state");
  st.className = `anim-state ${cls}`;
  st.textContent = cls === "over" ? `${phaseAt(s, t)[1]} · ${text}` : text;
  const scale = m.cap * 1.2;
  const fill = $("#c-gfill");
  fill.style.width = `${Math.max(0, Math.min(100, load / scale * 100))}%`;
  fill.style.background = load > m.cap ? "var(--critical)" : load > 0.85 * m.cap ? "var(--warn)" : "var(--s-grid)";
  $("#c-gnow").textContent = L.aNow(`${nf(load)} kW`, nf(load / m.cap * 100));
  $("#c-glim").textContent = L.aCap(`${nf(m.cap)} kW`);
  const squares = $("#c-grid").children;
  for (let i = 0; i < squares.length; i++) {
    const share = run.sites[i][k] / s.cap;
    squares[i].className = share < 0 ? "exp" : share < 0.85 ? "ok" : share <= 1 ? "near" : "hot";
    squares[i].title = `${i + 1}: ${nf(run.sites[i][k], 1)} kW`;
  }
}

function drawFeeder(s, run, base, m, k) {
  const meta = FEEDER_META;
  // A narrower drawing on phones keeps the text readable.
  const cw = $("#c-chart").clientWidth;
  const w = cw < 520 ? 420 : Math.max(760, Math.round(cw * 260 / 330)), h = 260, mg = { l: 48, r: 24, t: 12, b: 28 };
  $("#c-chart").setAttribute("viewBox", `0 0 ${w} ${h}`);
  const n = run.load.length, t0 = meta.from_h, dt = meta.sample_s / 3600;
  const hi = Math.ceil(Math.max(m.cap * 1.15, ...run.load, ...base.load) / 100) * 100;
  const lo = Math.min(0, Math.floor(Math.min(...run.load, ...base.load) / 100) * 100);
  const xs = (t) => mg.l + (t - t0) / (n * dt) * (w - mg.l - mg.r);
  const ys = (v) => mg.t + (hi - v) / (hi - lo) * (h - mg.t - mg.b);
  const parts = [];
  const end = REDUCTION_FROM + s.dur + (s.groups - 1) * 0.25;
  parts.push(`<rect x="${xs(REDUCTION_FROM)}" y="${mg.t}" width="${xs(end) - xs(REDUCTION_FROM)}" height="${h - mg.t - mg.b}" fill="${css("--dso")}" opacity=".09"/>`);
  parts.push(`<text x="${xs(REDUCTION_FROM) + 4}" y="${h - mg.b - 6}" style="fill:${css("--dso")};font-weight:600">${L.reductionLbl}</text>`);
  const step = hi - lo > 2000 ? 500 : hi - lo > 800 ? 200 : 100;
  for (let v = lo; v <= hi; v += step) {
    parts.push(`<line x1="${mg.l}" x2="${w - mg.r}" y1="${ys(v)}" y2="${ys(v)}" stroke="${css(v === 0 ? "--ink3" : "--hair")}" stroke-width="${v === 0 ? 1 : 0.6}"/>`);
    parts.push(`<text x="${mg.l - 6}" y="${ys(v) + 4}" text-anchor="end">${nf(v)}</text>`);
  }
  for (let t = 17; t <= 22; t++) parts.push(`<text x="${xs(t)}" y="${h - 8}" text-anchor="middle">${t}:00</text>`);
  const px = (j) => xs(t0 + (j + 0.5) * dt);
  const path = (load, upto) => load.slice(0, upto + 1).map((v, j) => `${j ? "L" : "M"}${px(j).toFixed(1)},${ys(v).toFixed(1)}`).join("");
  parts.push(`<line x1="${mg.l}" x2="${w - mg.r}" y1="${ys(m.cap)}" y2="${ys(m.cap)}" stroke="${css("--critical")}" stroke-width="1.5"/>`);
  parts.push(`<text x="${w - mg.r - 4}" y="${ys(m.cap) - 5}" text-anchor="end" style="fill:${css("--critical")}">${L.transformerLbl(`${nf(m.cap)} kW`)}</text>`);
  parts.push(`<path d="${path(base.load, n - 1)}" fill="none" stroke="${css("--ink2")}" stroke-width="1.5" stroke-dasharray="5 4"/>`);
  parts.push(`<path d="${path(run.load, k)}" fill="none" stroke="${css("--s-grid")}" stroke-width="2.2"/>`);
  parts.push(`<line x1="${px(k)}" x2="${px(k)}" y1="${mg.t}" y2="${h - mg.b}" stroke="${css("--ink3")}" stroke-width="1"/>`);
  const hot = run.load[k] > m.cap;
  parts.push(`<circle cx="${px(k)}" cy="${ys(run.load[k])}" r="5" fill="${css(hot ? "--critical" : "--s-grid")}" stroke="${css("--surface")}" stroke-width="2"/>`);
  $("#c-chart").innerHTML = parts.join("");
}

function renderFeederKpis(s, m) {
  const kwf = (v) => `${nf(Math.round(v))} kW`;
  const k = [
    [kwf(m.peakAfter), L.kPeak(nf(m.peakAfter / m.cap * 100))],
    [`${m.minutesOverAfter} min`, L.kOver(m.minutesOverAfter, m.minutesOver, m.baseMinutesOver)],
    [kwf(m.rebound), L.kRebound],
    [`${nf(Math.round(m.pushed))} kWh`, L.kPushed],
    [`${nf(Math.round(m.rise))} kW/min`, L.kRise],
    [`${nf(m.ev, 1)} kWh`, L.kShort(nf(m.evPerSite, 2))],
  ];
  $("#c-kpis").innerHTML = k.map(([b, t]) => `<div class="stat"><b>${b}</b><span>${t}</span></div>`).join("");
  const problems = [];
  if (m.minutesOverAfter > 0) problems.push(L.vOverload(m.minutesOverAfter));
  if (m.evPerSite > 0.1) problems.push(L.vShort(nf(m.ev, 1)));
  $("#c-verdict").innerHTML = `<b>${optionLabel(s)}:</b> ${problems.length ? `${problems.join(L.vJoin)}.` : L.vOk}`;
}

function mostViable(rows) {
  const ok = rows.filter((r) => r.m.evPerSite <= 0.1);
  const pool = ok.length ? ok : rows;
  return pool.slice().sort((a, b) => a.m.minutesOverAfter - b.m.minutesOverAfter || a.m.peakAfter - b.m.peakAfter || a.m.cost - b.m.cost)[0];
}

function renderCompare() {
  if (!calcRows.length) { $("#c-compare").hidden = true; return; }
  const best = mostViable(calcRows);
  const kw0 = (v) => nf(Math.round(v));
  const rows = calcRows.map((r, i) => `<tr data-i="${i}" class="${[r === best ? "best" : "", r === shown ? "shown" : ""].join(" ").trim()}">
    <td>${r === best ? "★ " : ""}${optionLabel(r.s)}</td>
    <td class="mono">${kw0(r.m.peakAfter)} (${nf(r.m.peakAfter / r.m.cap * 100)} %)</td>
    <td class="mono">${r.m.minutesOverAfter}</td>
    <td class="mono">${kw0(r.m.rebound)}</td>
    <td class="mono">${kw0(r.m.pushed)}</td>
    <td class="mono">${kw0(r.m.rise)}</td>
    <td class="mono">${nf(r.m.ev, 1)}</td>
    <td class="mono">${kw0(r.m.cost)} €</td></tr>`).join("");
  $("#c-table").innerHTML = `<thead><tr>${L.tHead.map((h) => `<th>${h}</th>`).join("")}</tr></thead><tbody>${rows}</tbody>`;
  $("#c-compare").hidden = false;
}

function wireCalculator() {
  for (const sel of ["#c-fleet", "#c-season", "#c-dur", "#c-release", "#c-groups", "#c-ctl"]) {
    onSeg(sel, (v) => press(sel, v));
  }
  onSeg("#c-season", () => $("#c-day-pick").classList.remove("on"));
  $("#c-day-pick").addEventListener("change", (e) => {
    if (!e.target.value) return;
    e.target.classList.add("on");
    press("#c-season", "");
  });
  $("#c-sites").addEventListener("input", syncCalc);
  $("#c-cap").addEventListener("input", syncCalc);
  syncCalc();
  $("#c-run").addEventListener("click", () => calculate([calcSettings()]));
  $("#c-table").addEventListener("click", (e) => {
    const tr = e.target.closest("tr[data-i]");
    if (!tr || calcBusy) return;
    showOption(calcRows[Number(tr.dataset.i)]);
  });
  $("#c-all").addEventListener("click", () => {
    const s = calcSettings();
    const list = Object.keys(RELEASE).map((release) => ({ ...s, release, groups: 1, ctl: "rules" }));
    list.push({ ...s, release: "wait10", groups: 4, ctl: "rules" });
    list.push({ ...s, release: "ramp", groups: 1, ctl: "mpc" });
    calculate(list);
  });
  $("#c-play").addEventListener("click", () => {
    if (!shown) return;
    if (!anim.playing && anim.pos >= shown.run.load.length - 1) { anim.pos = 0; anim.k = 0; }
    setAnimPlaying(!anim.playing);
    drawAnim();
  });
  onSeg("#c-speed", (v) => { anim.speed = Number(v); press("#c-speed", v); });
  $("#c-scrub").addEventListener("input", () => {
    if (!shown) return;
    setAnimPlaying(false);
    anim.k = anim.pos = Number($("#c-scrub").value);
    drawAnim();
  });
}

// The sidebar marks the section being read.
function wireSidebar() {
  const links = new Map([...document.querySelectorAll(".sidebar a.nav[href^='#']")].map((a) => [a.getAttribute("href").slice(1), a]));
  // The current section is the last one whose top has passed a third of the window.
  const mark = () => {
    let current = null;
    for (const id of links.keys()) {
      const el = document.getElementById(id);
      if (el && el.getBoundingClientRect().top < innerHeight / 3) current = id;
    }
    for (const [id, a] of links) a.classList.toggle("on", id === current);
  };
  addEventListener("scroll", mark, { passive: true });
  mark();
  addEventListener("resize", () => { fitCharts(); if (shown) drawAnim(); mark(); });
}

await init();
FEEDER_META = JSON.parse(feeder_meta());
collectText();
wire();
wireCalculator();
wireSidebar();
applyLanguage(pickLanguage());
if (!scripted()) start(6);
document.fonts?.ready.then(fitCharts);
initYear({
  L: () => L,
  nf,
  css,
  press,
  onSeg,
  locale: () => LOCALE[lang],
  // An evening of the year in the calculator: the study's feeder on that day.
  openInCalculator(date, cap, ctl) {
    $("#c-day-pick").value = date;
    $("#c-day-pick").classList.add("on");
    press("#c-season", "");
    $("#c-sites").value = "20";
    $("#c-cap").value = String(cap);
    press("#c-fleet", "mixed");
    press("#c-ctl", ctl);
    press("#c-release", "ramp");
    press("#c-groups", "1");
    press("#c-dur", "2");
    syncCalc();
    $("#calculator").scrollIntoView({ behavior: "smooth" });
    calculate([calcSettings()]);
  },
  openInDemo(date, ctl) {
    start(16.25, { season: date, strategy: ctl });
    $("#demo").scrollIntoView({ behavior: "smooth" });
  },
});
requestAnimationFrame(loop);
