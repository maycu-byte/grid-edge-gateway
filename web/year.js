// The year view: every evening of 2025 from docs/study/year2025 (bin/year),
// as a calendar coloured by what the transformer went through, and the
// evening of any day on click.

const $ = (s) => document.querySelector(s);
const MONTH_DAYS = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

let year = null, capIndex = 0, ctl = "rules", selected = null, ctx = null;

// Case indices in over_after / over_evening: rules_base, rules_ramp, mpc_base, mpc_ramp.
const cases = () => (ctl === "rules" ? [0, 1] : [2, 3]);

// k0 nothing to do · k1 the reduction solves it · k2 the rebound overloads
// again after the release · k3 over even with the reduction.
function classOf(day) {
  const [b, r] = cases();
  if (day.over_evening[capIndex][b] === 0) return "k0";
  if (day.over_after[capIndex][r] > day.over_after[capIndex][b]) return "k2";
  if (day.over_evening[capIndex][r] > 0) return "k3";
  return "k1";
}

function renderCalendar() {
  const L = ctx.L();
  const cells = [];
  let i = 0;
  MONTH_DAYS.forEach((n, m) => {
    cells.push(`<span class="m">${L.yMonths[m]}</span>`);
    for (let d = 0; d < 31; d++) {
      if (d >= n) { cells.push("<span></span>"); continue; }
      const day = year.days[i];
      const k = classOf(day);
      cells.push(`<button data-i="${i}" class="${k}${i === selected ? " sel" : ""}" title="${day.date}: ${L.yClass[k]}" aria-label="${day.date}: ${L.yClass[k]}"></button>`);
      i++;
    }
  });
  $("#y-cal").innerHTML = cells.join("");
  renderKpis();
}

function renderKpis() {
  const L = ctx.L(), nf = ctx.nf;
  const count = (k) => year.days.filter((d) => classOf(d) === k).length;
  const [need, rebound, still, solved] = [year.days.length - count("k0"), count("k2"), count("k3"), count("k1")];
  // the other controller, for comparison
  const other = ctl === "rules" ? "mpc" : "rules";
  const saved = ctl;
  ctl = other; const otherRebound = count("k2"); const otherNeed = year.days.length - count("k0"); ctl = saved;
  const k = [
    [nf(need), L.yNeed(nf(year.caps_kw[capIndex]))],
    [nf(rebound), L.yRebound],
    [nf(still), L.yStill],
    [nf(solved), L.ySolved],
  ];
  $("#y-kpis").innerHTML = k.map(([b, t]) => `<div class="stat"><b>${b}</b><span>${t}</span></div>`).join("")
    + `<p class="y-compare">${L.yCompare(other, nf(otherNeed), nf(otherRebound))}</p>`;
}

function renderDay() {
  if (selected == null) return;
  const L = ctx.L(), nf = ctx.nf, css = ctx.css;
  const day = year.days[selected];
  const [b, r] = cases();
  const key = ["rules_base", "rules_ramp", "mpc_base", "mpc_ramp"];
  const base = day[key[b]], ramp = day[key[r]];
  const cap = year.caps_kw[capIndex];
  const date = new Date(`${day.date}T12:00:00`).toLocaleDateString(ctx.locale(), { weekday: "long", day: "numeric", month: "long", year: "numeric" });
  const k = classOf(day);
  const facts = [
    [`${nf(day.temp_mean_c, 1)} °C`, L.yTemp],
    [`${nf(day.pv_kwh_per_kwp, 1)} kWh/kWp`, L.ySun],
    [`${nf(day.price_evening)} €/MWh`, L.yPrice],
  ];
  $("#y-day").innerHTML = `<h3>${date}</h3>
    <p><span class="anim-state ${{ k0: "", k1: "", k2: "over", k3: "rel" }[k]}">${L.yClass[k]}</span></p>
    <div class="legend"><span><i style="background:var(--s-grid)"></i>${L.yWith}</span><span><i class="dash"></i>${L.yWithout}</span><span><i style="background:var(--critical)"></i>${L.lgCap}</span></div>
    <svg id="y-chart" viewBox="0 0 600 240" role="img" aria-label="${day.date}"></svg>
    <div class="facts">${facts.map(([v, t]) => `<div class="stat"><b>${v}</b><span>${t}</span></div>`).join("")}</div>
    <p class="muted">${L.yDayNote(nf(base.ev_kwh, 1), nf(ramp.ev_kwh, 1), nf(ramp.peak_after))}</p>
    <div class="calc-actions"><button class="open" id="y-open-calc">${L.yOpenCalc}</button><button class="open" id="y-open-demo">${L.yOpenDemo}</button></div>`;
  drawDay(base.load, ramp.load, cap, css);
  $("#y-open-calc").addEventListener("click", () => ctx.openInCalculator(day.date, cap, ctl));
  $("#y-open-demo").addEventListener("click", () => ctx.openInDemo(day.date, ctl));
}

function drawDay(base, ramp, cap, css) {
  const w = 600, h = 240, mg = { l: 40, r: 24, t: 10, b: 26 };
  const n = base.length, t0 = year.from_h, dt = year.step_min / 60;
  const hi = Math.ceil(Math.max(cap * 1.15, ...base, ...ramp) / 20) * 20;
  const lo = Math.min(0, Math.floor(Math.min(...base, ...ramp) / 20) * 20);
  const xs = (t) => mg.l + (t - t0) / (n * dt) * (w - mg.l - mg.r);
  const ys = (v) => mg.t + (hi - v) / (hi - lo) * (h - mg.t - mg.b);
  const p = [];
  const [a, b] = year.reduction_h;
  p.push(`<rect x="${xs(a)}" y="${mg.t}" width="${xs(b) - xs(a)}" height="${h - mg.t - mg.b}" fill="${css("--dso")}" opacity=".09"/>`);
  const step = hi - lo > 160 ? 40 : 20;
  for (let v = lo; v <= hi; v += step) {
    p.push(`<line x1="${mg.l}" x2="${w - mg.r}" y1="${ys(v)}" y2="${ys(v)}" stroke="${css(v === 0 ? "--ink3" : "--hair")}" stroke-width="${v === 0 ? 1 : 0.6}"/>`);
    p.push(`<text x="${mg.l - 6}" y="${ys(v) + 4}" text-anchor="end">${v}</text>`);
  }
  for (let t = 17; t <= 22; t++) p.push(`<text x="${xs(t)}" y="${h - 8}" text-anchor="middle">${t}:00</text>`);
  const path = (l) => l.map((v, k) => `${k ? "L" : "M"}${xs(t0 + (k + 0.5) * dt).toFixed(1)},${ys(v).toFixed(1)}`).join("");
  p.push(`<line x1="${mg.l}" x2="${w - mg.r}" y1="${ys(cap)}" y2="${ys(cap)}" stroke="${css("--critical")}" stroke-width="2.2"/>`);
  p.push(`<path d="${path(base)}" fill="none" stroke="${css("--ink2")}" stroke-width="1.5" stroke-dasharray="5 4"/>`);
  p.push(`<path d="${path(ramp)}" fill="none" stroke="${css("--s-grid")}" stroke-width="2.2"/>`);
  p.push(`<text x="${mg.l + 4}" y="${mg.t + 12}">${ctx.L().yPerDepot}</text>`);
  $("#y-chart").innerHTML = p.join("");
}

function renderAll() {
  if (!year) return;
  ctx.press("#y-cap", String(year.caps_kw[capIndex]));
  ctx.press("#y-ctl", ctl);
  renderCalendar();
  renderDay();
}

export async function initYear(context) {
  ctx = context;
  try {
    const r = await fetch(`year2025.json?v=${document.documentElement.dataset.build}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    year = await r.json();
  } catch (e) {
    $("#y-day").innerHTML = `<p class="muted">${ctx.L().yFailed} (${e.message})</p>`;
    return;
  }
  capIndex = Math.max(0, year.caps_kw.indexOf(90));
  $("#y-cap").innerHTML = year.caps_kw.map((c) => `<button data-v="${c}" aria-pressed="false">${c} kW</button>`).join("");
  ctx.onSeg("#y-cap", (v) => { capIndex = year.caps_kw.indexOf(Number(v)); renderAll(); });
  ctx.onSeg("#y-ctl", (v) => { ctl = v; renderAll(); });
  $("#y-cal").addEventListener("click", (e) => {
    const b = e.target.closest("button[data-i]");
    if (!b) return;
    selected = Number(b.dataset.i);
    renderAll();
  });
  // start on the worst evening of the year for the rules
  selected = year.days.reduce((best, d, i) => (d.rules_ramp.peak_after > year.days[best].rules_ramp.peak_after ? i : best), 0);
  renderAll();
}

// Re-render after a language change.
export function refreshYear() {
  renderAll();
}
