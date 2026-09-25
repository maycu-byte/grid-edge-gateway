// Flexible connection: the site side of the capacity map (grid-solar-map).
// For each HV/MV district of the Saarland the regional study found the day of
// 2025 on which flexible plants lose the most, and the share of a flexible
// plant's output the substation allows in each hour. Played here, the grid
// operator turns that share into a feed-in limit sent over IEC 104 every five
// simulated minutes; the gateway keeps the export under it, using the energy
// on site and in the battery before it curtails the PV.

const $ = (s) => document.querySelector(s);
const MAP = "https://maycu-byte.github.io/grid-solar-map/";
const PV_KWP = 120;
const COMMAND_EVERY_S = 300;

let ctx = null, days = null, rule = "pro", active = null, lastDraw = 0;

const allowedOf = (d) => (rule === "pro" ? d.allowed_pro_rata : d.allowed_lifo_last);

export async function initFlex(context) {
  ctx = context;
  try {
    const r = await fetch(`flex_days.json?v=${document.documentElement.dataset.build}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    days = await r.json();
  } catch (e) {
    $("#f-note").textContent = `${ctx.L().fFailed} (${e.message})`;
    return;
  }
  const ids = Object.keys(days).sort((a, b) => days[a].name.localeCompare(days[b].name));
  $("#f-district").innerHTML = ids.map((id) => `<option value="${id}">${days[id].name}</option>`).join("");
  const wanted = new URLSearchParams(location.search).get("district");
  // default: the district where flexible plants lose the most on their worst day
  const worst = ids.reduce((a, b) => (days[b].day_loss_pro_rata > days[a].day_loss_pro_rata ? b : a));
  $("#f-district").value = wanted && days[wanted] ? wanted : worst;
  if (wanted && days[wanted]) $("#flex").scrollIntoView();
  $("#f-district").addEventListener("change", render);
  ctx.onSeg("#f-rule", (v) => { rule = v; ctx.press("#f-rule", v); render(); });
  $("#f-play").addEventListener("click", play);
  render();
}

function play() {
  const id = $("#f-district").value, d = days[id];
  ctx.start(6, { season: d.date, country: "DE", flex: { id, allowed: allowedOf(d), pct: null, lastCmd: -Infinity, curtKwh: 0, availKwh: 0 } });
  $("#demo").scrollIntoView({ behavior: "smooth" });
}

// Called by the demo when a day starts: a flexible day, or any other (which ends the mode).
export function flexStart(opts) {
  active = opts ?? null;
  render();
}

export function flexTick(simSeconds) {
  if (!active) return;
  const s = ctx.state();
  const h = Math.floor(s.t_s / 3600) % 24;
  const share = active.allowed[h];
  if (s.t_s - active.lastCmd >= COMMAND_EVERY_S) {
    // the share applies to what the sun gives now; 100% means no limit at all
    const pct = share >= 0.999 ? 100 : Math.max(0, Math.min(100, Math.round((share * s.pv_available_kw) / PV_KWP * 100)));
    if (pct !== active.pct) {
      ctx.demo().command_feed_in(pct);
      ctx.press("#feed-seg", String(pct));
      active.pct = pct;
    }
    active.lastCmd = s.t_s;
  }
  if (simSeconds > 0) {
    active.availKwh += s.pv_available_kw * simSeconds / 3600;
    active.curtKwh += Math.max(0, s.pv_available_kw - s.pv_kw) * simSeconds / 3600;
  }
  const now = performance.now();
  if (now - lastDraw > 250) { lastDraw = now; render(); }
}

export function refreshFlex() { if (days) render(); }

function render() {
  if (!days || !ctx) return;
  const L = ctx.L(), nf = ctx.nf;
  const id = active ? active.id : $("#f-district").value;
  const d = days[id];
  const allowed = active ? active.allowed : allowedOf(d);
  const date = new Date(`${d.date}T12:00:00`).toLocaleDateString(ctx.locale(), { day: "numeric", month: "long", year: "numeric" });
  $("#f-note").innerHTML = L.fNote(d.name, date, nf(d.flexible_mw), nf(d.firm_mw),
    nf(d.day_loss_pro_rata * 100), nf(d.day_loss_lifo_last * 100));
  $("#f-map").href = `${MAP}?lang=${ctx.lang()}`;
  drawChart(allowed);
  const s = ctx.state();
  const onDay = active && s && s.t_s < 30 * 3600;
  const hour = onDay ? Math.floor(s.t_s / 3600) % 24 : null;
  const lost = active && active.availKwh > 0 ? active.curtKwh / active.availKwh : 0;
  $("#f-stats").innerHTML = active ? [
    [`${nf(allowed[hour ?? 0] * 100)}%`, L.fAllowedNow],
    [`${active.pct ?? 100}%`, L.fLimitSent],
    [`${nf(active.curtKwh, 1)} kWh`, L.fCurtailed(nf(lost * 100, 1))],
    [`${nf(s.battery.soc_pct)}%`, L.fBattery],
  ].map(([b, t]) => `<div class="stat"><b>${b}</b><span>${t}</span></div>`).join("") : `<p class="muted">${L.fIdle}</p>`;
  $("#f-play").textContent = active && active.id === $("#f-district").value ? L.fReplay : L.fPlay;
}

function drawChart(allowed) {
  const css = ctx.css, w = 600, h = 170, m = { l: 40, r: 8, t: 10, b: 24 };
  const bw = (w - m.l - m.r) / 24;
  const y = (v) => m.t + (1 - v) * (h - m.t - m.b);
  const s = ctx.state();
  const cur = active && s && s.t_s < 30 * 3600 ? Math.floor(s.t_s / 3600) % 24 : -1;
  const p = [];
  for (const v of [0, 0.5, 1]) {
    p.push(`<line x1="${m.l}" x2="${w - m.r}" y1="${y(v)}" y2="${y(v)}" stroke="${css("--hair")}" stroke-width="0.6"/>`);
    p.push(`<text x="${m.l - 6}" y="${y(v) + 4}" text-anchor="end">${v * 100}%</text>`);
  }
  allowed.forEach((v, i) => {
    const colour = v >= 0.999 ? css("--good") : v >= 0.5 ? css("--warn") : css("--critical");
    p.push(`<rect x="${m.l + i * bw + 1}" y="${y(v)}" width="${bw - 2}" height="${h - m.b - y(v)}" fill="${colour}" opacity="${i === cur ? 1 : 0.7}"${i === cur ? ` stroke="${css("--ink")}" stroke-width="1.5"` : ""}/>`);
    if (i % 3 === 0) p.push(`<text x="${m.l + i * bw + bw / 2}" y="${h - 8}" text-anchor="middle">${String(i).padStart(2, "0")}:00</text>`);
  });
  $("#f-chart").innerHTML = p.join("");
}
