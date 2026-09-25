# Grid Edge Gateway

**A site controller in Rust between a grid operator and a prosumer site in Germany, Austria or Switzerland: IEC 60870-5-104 towards the DSO, SunSpec Modbus towards PV, chargers, heat pump and battery, each country's rules for dimming and feed-in limits in between — and a model-predictive planner on top that schedules the site against live day-ahead prices (ENTSO-E or Energy-Charts), a weather forecast (Open-Meteo), a demand charge, departure times and the building's thermal mass.**

**[Open the live demo →](https://maycu-byte.github.io/grid-edge-gateway/)** The demo runs this code in your browser, compiled to WebAssembly — the planner's quadratic program included. You play the grid operator: pick the country, the day and the controller, send the commands and watch the site respond, frame by frame, next to a copy of the same site running on rules alone.

**Feeder calculator.** Below the demo, a calculator answers the question the rebound study asks: when a §14a reduction ends for many sites at once, which way of bringing them back is most viable? Pick the number of sites, the transformer, identical or different van timetables, the day, the length of the reduction, the release policy (at once, the German 5-minute ramp, a random wait of up to 10 or 30 minutes, a 30-minute ramp, release in groups) and the site controller. The page simulates every site with the same code as the study (`closedloop::feeder`) and the same evening without a reduction, then reports the peak after the release, minutes over the transformer, the rebound, the energy pushed later and the energy the vans are left without, and marks the most viable option. With the defaults, every rule-based release brings the transformer back to overload, a slower restart only makes the rise gentler, and the price-aware planner stays at half the transformer with every van charged. The evening then plays back minute by minute: the feeder curve, the release phase, a gauge against the transformer and one square per site, with play, speed and a time slider.

The page is available in English, Portuguese and German (switch at the top right; it follows the browser language by default). A sidebar leads to each section; a first section explains what is simulated, and a data-sources section lists what is real (German day-ahead prices from SMARD, the BNetzA and national rules) and what is modelled (weather, vans, building, tariff), and why the demo uses two representative days.

![A winter day under the planner, on the real prices of 20 January 2025: the battery charges at noon and discharges into the evening price peak, the building is warmed before the §14a dimming, the overnight vans charge at a flat 42 kW. Against the same site on rules alone: 76 € less, and a peak of 42 kW instead of 110 kW](docs/screenshot-day.png)

## The problem

Distribution grids in Central Europe now have to control what happens behind the meter. Rooftop PV peaks at noon, and heat pumps and EV chargers peak in the evening, on low-voltage grids that were built to deliver power one way. Each country gives its DSOs a different set of rights:

| | Consumption | Feed-in | Source |
|---|---|---|---|
| **Germany** | §14a EnWG: the DSO may dim heat pumps, chargers and batteries, but must leave a guaranteed minimum, *Pmin,14a* | DSO setpoints for plants over 100 kW (EEG §9); new systems without smart meter capped at 60% (Solarspitzengesetz, 2025) | [BNetzA BK6-22-300, Anlage 1](https://www.bundesnetzagentur.de/DE/Beschlusskammern/1_GZ/BK6-GZ/2022/BK6-22-300/Beschluss/BK6-22-300_Beschluss_Anlage1.pdf?__blob=publicationFile&v=1) |
| **Austria** | No statutory minimum like §14a was found in the sources checked; modelled as a flexibility contract | *Spitzenkappung*: the DSO may cap feed-in of new or extended PV at up to 70% of module peak power; a dynamic version is planned for 2028 | ElWG (BGBl. I 91/2025); [BMWET factsheet](https://www.bmwet.gv.at/dam/jcr:260fda60-c745-4861-bdcb-1c1223a4af58/ElWG-Factsheets_Spitzenkappung_final%20final.pdf) |
| **Switzerland** | Flexibility is used by contract, with pay; the owner may forbid uses that existed before 2026 | Guaranteed, unpaid curtailment of **at most 3% of the yearly energy** at the connection point; unlimited in an immediate, serious threat | [StromVG Art. 17c](https://www.fedlex.admin.ch/eli/cc/2007/418/de), StromVV Art. 19a–19d (in force 1 Jan 2026) |

The box on site has to speak the DSO's protocol on one side and the devices' protocols on the other, apply the right country's rules, and stay safe when a link, a device or the box itself fails. This project builds that box, with one controller and three rule sets.

## The demo site

A logistics depot with **120 kWp** rooftop PV (two 60 kW inverters), **four 22 kW chargers** for delivery vans, a **14 kW heat pump** and a **100 kWh / 50 kW battery**.

```
DSO control centre ──IEC 104 / TLS──► grid-edge-gateway ──Modbus TCP / SunSpec──► inverters, meter, chargers, heat pump, battery
      (SCADA)                           (this repository)                         (site-sim, or real devices)
```

## What the gateway does

| | |
|---|---|
| **Consumption dimming** | **DE:** Pmin,14a exactly as in BK6-22-300 Anlage 1, 4.5.2: `max(0.4·ΣP_WP, 0.4·ΣP_Klima) + (n−1)·GZF·4.2 kW` with a heat pump above 11 kW, else `4.2 kW + (n−1)·GZF·4.2 kW`. For the depot (n = 6: four chargers, the battery, the heat pump; GZF 0.6): **18.2 kW**. **AT/CH:** the contract's minimum power and maximum minutes per day; devices whose owner opted out (CH) are never limited. In both cases the limit applies to power *drawn from the grid*, so PV surplus and battery discharge come on top. |
| **Allocation** | Each heat pump first gets 40% of its rating. Then as many cars as fit get the IEC 61851 minimum of 6 A: the car with the least slack before its departure first (time left minus time to charge at the car's own maximum current, as ISO 15118 reports it), then the least-charged. What is left tops up the heat pump, then the cars in whole amps. Values are always rounded *down*: when the exact value is impossible, the regulation asks for the next lower one. Cars are only rotated after a 5-minute dwell time. |
| **Feed-in limits** | The DSO setpoint, capped by the country's standing limit (AT 70%, DE 60% where it applies), applied at the grid connection: PV may produce more while the site consumes or stores it. In CH the gateway counts curtailed energy against the 3% budget and refuses non-emergency curtailment beyond it. |
| **Battery** | Stores what would be exported, so a feed-in limit is absorbed before any PV is curtailed; covers imports; while dimmed, discharges so the loads keep more of their budget. It never charges from the grid while dimmed. With a plan, it holds the planned grid exchange instead and so absorbs the forecast errors. |
| **Following a plan** | The planner's schedule is *guidance*. Chargers are capped at the planned current, but a car whose slack drops under 15 minutes charges at full power whatever the plan says; the heat pump gets the planned power as an external request (SG Ready / EEBUS style) and keeps its own comfort guard; every hard rule above is applied after the plan. A property test feeds 10,000 random plans during a dimming and checks that the floor, the 0 or 6–32 A currents in whole amps and the heat-pump cap still hold. |
| **Emergency** | A separate command for an immediate, serious threat (StromVG 17c 4b): overrides day limits, budgets and opt-outs. |
| **Gradual release** | After a dimming ends, power returns linearly over 5 minutes (BK6-22-300, 4.3); a raised feed-in limit returns at about 10% of installed power per minute. Optionally the site first waits a random 0–`release_delay_max_s` seconds, so that sites released by the same command do not ramp up together (the UK asks for up to 600 s: [SI 2021/1467](https://www.legislation.gov.uk/uksi/2021/1467/part/2/made)). |
| **Proof of each dimming** | When a dimming ends, the gateway writes a CSV report: the floor, the controllable devices' grid draw every second, the time above the floor after a 60 s settling time, and a verdict (followed, exceeded, unverified). Each report names the SHA-256 of the one before, so a report edited or deleted later breaks the chain. The operator must be able to show this to the DSO and keep it for two years ([Anlage 1](https://www.bundesnetzagentur.de/DE/Beschlusskammern/1_GZ/BK6-GZ/2022/BK6-22-300/Beschluss/BK6-22-300_Beschluss_Anlage1.pdf?__blob=publicationFile&v=1), 7.2–7.3). Reports go to `reports/` and are served at `/api/reports`. |
| **Inverters that ignore their limit** | Each inverter's output is compared with the limit it was sent. One still above it after `pv_follow_timeout_s` (30 s) is reported to the DSO (point 2008), its output is taken as given, and the other inverters are limited further so the plant as a whole keeps to the limit. |

## IEC 104 point list (common address 1)

| IOA | Type | Direction | Meaning |
|---|---|---|---|
| 5001 | C_SC_NA_1 | DSO → site | Reduce consumption ON / OFF (§14a in DE, contract in AT/CH) |
| 5002 | C_SE_NC_1 | DSO → site | Feed-in limit, % of installed PV (0–100; other values get a negative confirmation) |
| 5003 | C_SC_NA_1 | DSO → site | Emergency ON / OFF |
| 1001 | M_ME_TF_1 | site → DSO | Active power at the grid connection, kW (+ import) |
| 1002 | M_ME_TF_1 | site → DSO | PV active power, kW |
| 1003 | M_ME_TF_1 | site → DSO | Controllable devices (steuVE), kW |
| 1004 | M_ME_TF_1 | site → DSO | steuVE power drawn from the grid, kW: the quantity a consumption limit applies to |
| 1005 | M_ME_TF_1 | site → DSO | Consumption floor while dimmed, kW (Pmin,14a in DE) |
| 1006 | M_ME_TF_1 | site → DSO | Feed-in limit commanded, % (feedback of 5002) |
| 1007 | M_ME_TF_1 | site → DSO | PV limit sent to the inverters, % |
| 1008 | M_ME_TF_1 | site → DSO | Feed-in limit in force after country rules, % |
| 1009 | M_ME_TF_1 | site → DSO | Battery power, kW (+ charging) |
| 1010 | M_ME_TF_1 | site → DSO | Battery state of charge, % |
| 1011 | M_ME_TF_1 | site → DSO | Consumption dimmed today, minutes |
| 1012 | M_ME_TF_1 | site → DSO | PV energy curtailed this year, kWh |
| 1013 | M_ME_TF_1 | site → DSO | Free curtailment budget used, % (CH) |
| 2001 | M_SP_TB_1 | site → DSO | Consumption dimming active (feedback of 5001) |
| 2002 | M_SP_TB_1 | site → DSO | Gradual release in progress |
| 2003 | M_SP_TB_1 | site → DSO | Meter fallback (grid measurement lost) |
| 2004 | M_SP_TB_1 | site → DSO | Field device fault |
| 2005 | M_SP_TB_1 | site → DSO | Emergency active (feedback of 5003) |
| 2006 | M_SP_TB_1 | site → DSO | Contract day limit reached, dimming refused |
| 2007 | M_SP_TB_1 | site → DSO | Curtailment budget used up |
| 2008 | M_SP_TB_1 | site → DSO | An inverter ignores its limit (the others are limited further) |

Commands support direct execute and select-before-operate. Unknown addresses, types, causes and common addresses get the negative confirmations the standard defines (causes 44–47). Measurements are sent spontaneously outside a deadband and all together in a general interrogation.

![§14a dimming at 18:18, at the price peak: the plan lets the overnight vans wait for cheaper hours and charges the van that leaves at 20:30 at 12 A; the heat pump is off because the building was warmed at noon; the battery discharges 16 kW; the IEC 104 reports go out](docs/screenshot-dimming.png)

## Robustness

| Failure or edge case | What happens |
|---|---|
| Gateway crashes or hangs | Each charger's watchdog (armed at start) drops it to **6 A = 4.14 kW** after 30 s, below 4.2 kW on its own. The battery's BMS watchdog idles it. Inverter limits are written with **no revert timeout** (SunSpec `WMaxLimPct_RvrtTms = 0`), so the last DSO limit stays in force. |
| Gateway restarts | DSO commands and the running totals (minutes dimmed today, energy curtailed this year) are persisted with write-then-rename, so a dimming or a used-up budget survives a power cut. |
| Grid meter lost **or implausible** | A reading far beyond the connection rating (e.g. a wrong scale factor) counts as lost. The budget shrinks to the floor; the feed-in limit falls back to plant-output mode. Point 2003 goes ON. |
| A device stops answering | Assumed to draw its failsafe current (charger) or rated power (heat pump); a silent battery is assumed idle. The budget for the others shrinks accordingly. |
| An inverter answers but ignores its limit | Reported after 30 s (2008 and 2004 go ON); the other inverters take over its share of the limit. |
| Battery and curtailment chase each other | The battery plans from the PV the sun *allows* (vendor SunSpec model 64900), so a curtailment never looks like a deficit to discharge into. Without that reading it never discharges under a feed-in limit. |
| DSO link lost | Configurable: `hold` the last commands (default) or `release` them after a set time. An emergency always stays until the DSO clears it. |
| A contract's day limit or a budget is used up | The command is refused, the refusal is reported (2006 / 2007), and an emergency still overrides it. |
| Control loop too slow | Cycles longer than the period are counted and logged; the cycle time is in the API. |
| Configuration mistakes | Validated at start: unknown country, contract settings for DE, caps outside 0–100%, a failsafe current a car would not accept, and more. |
| Price or weather feed down | The planner falls back to the other price source, then to the last prices it has (yesterday's for hours not yet published) and the last forecast. Without any prices it stops planning and the site runs on rules. The error shows in `/api/snapshot`. |
| Someone else reaches port 2404 | With TLS on, the station accepts only client certificates signed by the DSO's CA (TLS 1.2/1.3, in the spirit of IEC 62351-3). |

## The planning layer

Rules keep the site legal; they do not make it cheap. On top of the real-time controller, a model-predictive planner solves the site's next 24 hours every 15 minutes (and whenever a car plugs in). It is a convex quadratic program over 96 quarter-hours with:

- battery losses and ageing;
- a thermal model of the building;
- each car's energy request and departure time;
- the DSO's announced dimming window;
- real day-ahead prices;
- a demand charge on the highest quarter-hour.

It takes about 16 ms to solve, in Rust: in the gateway, in the study and in the browser. The real-time layer follows the plan only as far as the rules allow.

A Monte Carlo study compares it with plain rules on the depot: 30 random-weather days each in spring and winter, on real German day-ahead prices, with the DSO dimming from 17:30 to 19:30.

| Per day | Spring (6 Apr 2025) | Winter (20 Jan 2025) |
|---|---|---|
| Rules: energy + battery ageing + peak charge | 126.6 € | 357.9 € |
| MPC | **−25.6 ± 2.2 €** (−20%) | **−59.7 ± 2.4 €** (−17%) |
| Highest quarter-hour of import | 89 → 34 kW | 110 → 57 kW |
| MPC without the demand charge | −15.6 €, and a *higher* peak (93 kW) | −44.4 €, peak 118 kW |
| Chance-constrained / robust MPC | same as MPC | same as MPC |

In every run, every car left with the energy it asked for. The value comes from prices, the peak and the building's thermal mass, not from hedging forecast errors. The real-time layer absorbs those errors, and §14a guarantees a floor. With an afternoon dimming, the chance-constrained reserve cost 5 € a day in spring, and its only return was a few seconds less above the floor. The formulation, the study design, all the numbers and the limits are in **[docs/mpc.md](docs/mpc.md)**.

### In the gateway

With a `[planner]` section in its configuration ([example](examples/gateway-de-planner.toml)), the gateway plans on live data. A background task does the work; the 1 s control loop only reads the plan's guidance for the current quarter-hour, which takes about 0.1 ms, and never waits for the network or the solver. The task:

- **Prices.** It fetches day-ahead prices for the bidding zone (DE-LU, AT or CH) from the [ENTSO-E Transparency Platform](https://transparency.entsoe.eu/). Without an ENTSO-E token it uses [Energy-Charts](https://api.energy-charts.info/) (Fraunhofer ISE, SMARD data). DE-LU and AT trade 15-minute products since October 2025; Switzerland stays hourly. Tomorrow's prices are published around 13:00.
- **Weather.** It fetches [Open-Meteo](https://open-meteo.com/)'s forecast of irradiance on the plane of the modules and outdoor temperature, in 15-minute steps. It turns that into PV power and corrects it with a nowcast from what the inverters report.
- **Learning and metering.** It learns the site's base load for each quarter-hour of working days and weekends. It meters the billing period's highest quarter-hour for the demand charge.
- **Re-planning.** It re-plans at every quarter-hour, and when a car plugs in or leaves, when the DSO starts or ends a dimming, and when new prices arrive.
- **Serving and keeping state.** It serves the plan at `/api/plan` and its status in `/api/snapshot`. Across restarts it keeps what it learned, the billing peak, and the last prices and forecast.

Without prices, or with a plan older than an hour, the site runs on rules alone. The rules never depend on the plan.

## How it is built

| Crate | What it is |
|---|---|
| [`iec104`](crates/iec104) | IEC 60870-5-104 from scratch, with no dependencies: APDU framing, the ASDUs above plus clock sync and interrogation, CP56Time2a, and the controlled-station link layer (k/w windows, t1/t2/t3 timers, 15-bit sequence numbers) as a **sans-IO state machine**, so every timing rule is unit-tested without sleeping. An optional tokio driver runs it over TCP or TLS. |
| [`control`](crates/control) | The real-time layer: country policies (`policy.rs`), running totals (`accounting.rs`), the compliance recorder (`compliance.rs`) and the controller, pure functions and a small state machine. It takes a plan as guidance and enforces every rule after it. The same code runs in the gateway, in the tests and in the browser. |
| [`planner`](crates/planner) | The planning layer: the site's next 24 hours as a convex quadratic program, solved with [Clarabel](https://github.com/oxfordcontrol/Clarabel.rs) (interior point, pure Rust, also in WebAssembly). Battery with losses and ageing, a first-order thermal model of the building, each car's request, the expected dimming window, a demand charge, and deterministic, chance-constrained or robust handling of forecast errors. See [docs/mpc.md](docs/mpc.md). |
| [`planning`](crates/planning) | The glue between the site and the optimiser, shared by the gateway, the study and the browser, so all three plan the same way. It builds the planner's input from what the devices report, turns a plan into guidance for the real-time layer, learns the base-load profile, meters the quarter-hour peak, and keeps Central European time, summer time included, without a time-zone database. |
| [`closedloop`](crates/closedloop) | The simulated depot, the register adapter, the real-time controller and the planner with its forecaster, wired into one loop; and the [`study`](crates/closedloop/src/bin/study.rs) binary, a Monte Carlo comparison of the strategies. |
| [`devices`](crates/devices) | SunSpec register layouts (models 1, 103, 120, 123, 203 and a vendor model), typical wallbox, heat-pump and battery maps with watchdogs, and a deterministic simulation of the depot over several days: PV under random cloudiness, the building's heat balance, battery losses, a van fleet with arrival and departure times, and real German day-ahead prices for a spring and a winter day. |
| [`gateway`](crates/gateway) | The binary: one supervised task per Modbus device (SunSpec discovery by walking the model chain, reconnect, staleness detection), a 1 s control loop, the IEC 104 station, TLS (rustls), persistence, the compliance reports on disk and a read-only JSON/WebSocket API. The planning layer's live feeds are here too: ENTSO-E and Energy-Charts day-ahead prices, and the Open-Meteo forecast. |
| [`site-sim`](crates/site-sim) | Every device of the depot as its own Modbus TCP server, with an HTTP endpoint to take devices offline. With `--start now`, its sun is at today's hour, for runs against live prices and weather. |
| [`web-demo`](crates/web-demo) | The browser build: the closed loop (planner included) + IEC 104 encoder in WebAssembly, with a rules-only copy of the site alongside for comparison. The controller reads and writes the simulated devices through their register maps, like the gateway does over Modbus TCP. |

## Testing

- **157 Rust tests.** They cover:
  - protocol frames checked against reference octets, every link-layer timer and window, sequence-number wrap-around;
  - the Pmin formula for several device mixes, allocation scenarios, each country's rules, day and year roll-over of the totals, the battery, plausibility checks, ramps and config validation;
  - three property tests over 45,000 random site states and plans. While dimmed, in every country, with a battery and whatever the plan says, the loads never get more than the floor + PV surplus + battery discharge. Every charger current is 0 or 6–32 A in whole amps;
  - the planner: price-driven battery use, charging before departure in the cheapest hours, pre-heating before a dimming, the dimming constraint, the demand charge, uncertainty reserves, a full day with four cars;
  - how the real-time layer follows a plan: least slack first, the departure guard, the heat pump's request, a passing cloud during a dimming;
  - the closed loop: every strategy through an evening dimming, and the planner saving money without raising the peak;
  - live planning in the gateway:
    - ENTSO-E documents (curve type A03, 15-minute and hourly products, error answers), and Energy-Charts and Open-Meteo answers;
    - the price book's fallbacks and PV power from irradiance;
    - the nowcast and base-load learning;
    - billing-peak metering and restarts;
    - Central European summer time;
  - the checks around a command: the random wait before power returns, an inverter that ignores its limit (reported after the timeout, the other one makes up for it), and the compliance reports (verdicts, the SHA-256 chain, a forged value breaking it, the chain continuing after a restart, file names that cannot escape the report folder);
  - the site simulation: register maps and watchdogs, the thermostat and an EMS taking it over (with the heat pump's own comfort guard), departures and unmet energy, day-to-day weather, prices in local time.
- **20 interoperability tests** ([`interop/`](interop/test_interop.py)). They start the real simulator and gateway and drive them with [c104](https://github.com/Fraunhofer-FIT-DIEN/iec104-python), a Python binding of lib60870, as the DSO control centre. They cover:
  - general interrogation, §14a compliance within seconds, gradual release and negative confirmations;
  - meter loss, a battery gone silent, the emergency command and the link-loss policy;
  - persistence across a restart, the Austrian 70% cap and the Swiss 3% budget running out;
  - the planner in the running gateway: it moves the overnight vans' charging to cheap hours, and the Pmin floor holds when the DSO dims;
  - two dimmings leaving two chained reports on disk and at `/api/reports`, and an inverter that ignores its limit being reported over IEC 104;
  - TLS acceptance and rejection.
- **77 browser checks** of the live page with Playwright, in English, Portuguese and German at 1366 px, 1920 px (dark mode) and 390 px: nothing left untranslated, no horizontal scroll, the calculator's output and its minute-by-minute playback (clock, one square per site, time slider), the height difference between side-by-side columns, switching language while the demo runs, no console errors. They were run by hand for this version; CI does not run them yet.
- CI runs `fmt`, `clippy -D warnings`, all tests and the interop suite on every push, then builds the WebAssembly demo and deploys it to GitHub Pages.

## Run it locally

Needs Rust (stable) and, for the interop tests, Python 3.10+.

```sh
cargo build
./target/debug/site-sim --start 17:30                  # the depot, Modbus TCP on :5020–5028
./target/debug/gateway gateway.toml                    # Germany; IEC 104 on :2404, API on :8080
./target/debug/gateway examples/gateway-ch.toml        # or Switzerland (examples/gateway-at.toml: Austria)
```

Then act as the DSO with any IEC 104 master (address 1, IOA 5001 / 5002 / 5003), and read the reports of past dimmings with `curl localhost:8080/api/reports`. Or run the tests:

```sh
pip install -r interop/requirements.txt
sh certs/gen-demo.sh                           # throw-away PKI for the TLS tests
python -m pytest -v interop
```

To enable TLS, uncomment `[iec104.tls]` in `gateway.toml`. For the browser demo, run `web/build.sh`, which needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` 0.2.128.

The gateway with the planner, on live prices and weather:

```sh
./target/debug/site-sim --start now                                        # the simulated sun at today's hour
ENTSOE_TOKEN=... ./target/debug/gateway examples/gateway-de-planner.toml   # without the token: Energy-Charts
curl localhost:8080/api/plan                                               # the plan in force
```

To get an ENTSO-E token:

1. Register on the Transparency Platform.
2. Ask for API access by e-mail to transparency@entsoe.eu, with "Restful API access" as the subject.
3. The token then appears in your account settings.

The token is read only from the environment and never written to a log.

The rebound study behind the feeder calculator (20 sites × 10 days × 33 cases: identical and mixed fleets, release in groups, 1–3 h reductions, the planner with and without notice, spring; about 30 minutes):

```sh
cargo run --release -p closedloop --bin rebound -- --sites 20 --reps 10     # → docs/study/rebound
```

The planner study (a few minutes on a laptop):

```sh
cargo run --release -p closedloop --bin study -- --seeds 30                               # → docs/study
cargo run --release -p closedloop --bin study -- --seeds 30 --dim 13-15 --out docs/study/afternoon
cargo run --release -p closedloop --bin study -- trace winter 1 mpc trace.csv             # one run, every 5 min
```

## Where this fits, and what is still missing

In a German home, the hardware towards the DSO is already mandated and price-capped by law: a smart meter gateway plus an FNN control box. Together they cost the customer at most about 80–100 € a year. The flat §14a grid-fee reduction ("Modul 1", 110–190 € a year) pays for that on its own. What remains open is the software between the control box and the devices:
- splitting the guaranteed minimum among several devices;
- proving afterwards that every dimming was followed;
- using time-variable grid fees ("Modul 3") and day-ahead prices.

Open-source energy managers such as [evcc](https://docs.evcc.io/en/external-limit/) already take the control box's signal over a relay or EEBUS LPC. This project explores the parts around that signal: Pmin,14a with the simultaneity factor for several devices, gradual release, proof of each dimming, and a predictive planner.

A review against the regulation found five gaps, in order of value. Three are closed:

| Gap | Why it matters | Status |
|---|---|---|
| **A report of each dimming.** | The site operator must be able to show the DSO, case by case, that each reduction was carried out, and keep that for 2 years ([BK6-22-300 Anlage 1](https://www.bundesnetzagentur.de/DE/Beschlusskammern/1_GZ/BK6-GZ/2022/BK6-22-300/Beschluss/BK6-22-300_Beschluss_Anlage1.pdf?__blob=publicationFile&v=1), 7.2–7.3, since March 2025). | Done: a CSV per dimming, chained by SHA-256. The chain shows tampering but proves nothing about who wrote it; a real device would sign with a key in a secure element. |
| **An inverter that ignores its limit.** | A silent failure otherwise. The operator must keep devices controllable at all times (Anlage 1, 4.6). | Done: reported (point 2008), the others make up for it. |
| **A random wait before the release ramp.** | Without it, every site that ends a dimming at the same moment ramps up together: a second peak in the neighbourhood. The UK requires up to 600 s ([SI 2021/1467](https://www.legislation.gov.uk/uksi/2021/1467/part/2/made)). | Done, off by default in the gateway (`release_delay_max_s`); on in the demo. |
| **An EEBUS LPC interface.** | EEBUS LPC is how German households receive §14a behind an FNN control box. DSO commands arrive only over IEC 104 here. | Open. |
| **Re-planning on forecast error.** | A sudden drop in PV is absorbed by the real-time layer but does not trigger a new plan, which stays suboptimal until the next quarter-hour. | Open. |

## Limits and honest notes

- **Rules, not legal advice.** The country rules are taken from the texts linked above as of September 2026. The Austrian rules were read from the ministry's factsheet and secondary sources, because the official legal database blocks automated access; no Austrian consumption-side minimum was found. DSOs' technical connection rules add details not modelled here.
- **Simulated devices.** The wallbox, battery and heat-pump register maps follow common patterns but are not a specific product's map; the available-power register is a vendor model, as on real inverters. SunSpec layouts follow the published models.
- **Signal path.** In German households the §14a signal usually travels through the smart meter gateway (CLS channel) to an FNN control box or via EEBUS. IEC 104 is the standard telecontrol path for larger plants (from 100 kW). This project uses IEC 104 for all commands to keep one DSO interface; the control logic does not depend on the transport.
- **Protocol scope.** The IEC 104 stack implements the subset a controlled station of this kind needs, not the full companion standard (no file transfer, no redundancy groups). It is not certified; it is tested against lib60870.
- **Clock.** Day and year boundaries for the running totals use UTC.
- **Live planning is simple where it can be.**
  - **PV model.** One plane of modules per site, with a performance ratio and a temperature derating, not a full PV model.
  - **Base load.** The profile needs a few days to learn; until then, the plan assumes the load now continues.
  - **Building.** The UA and capacity have to be set from data.
  - **Mid-quarter plans.** A plan made between quarter-hours treats the current quarter as a whole one.
  - **Unpublished prices.** Until tomorrow's prices are published (around 13:00), the plan assumes yesterday's for those hours.
  - **Tests.** The ENTSO-E parser is tested against documents in the published format, not a live answer: this repository has no token.
  - **Weather licence.** The free Open-Meteo API is for non-commercial use only ([terms](https://open-meteo.com/en/terms)); a commercial deployment needs a paid plan, and a fleet should fetch one forecast per grid cell centrally rather than per site.
- **Planner study.** The simulated building is the planner's own model (same thermal parameters), so model mismatch comes only from the forecasts; a real building would need its parameters identified first, and the gains would shrink. The rules baseline is plain (a fixed thermostat schedule with no optimum start, cars at full power on arrival). See [docs/mpc.md](docs/mpc.md#limits) for the rest.
- **Not a product.** Four EU rules would apply to a device like this on the market:
  - the Radio Equipment Directive's cybersecurity requirements (EN 18031, since August 2025, if it has a radio);
  - the Data Act (user access to device data, for products placed on the market from 12 September 2026);
  - the new Product Liability Directive, which covers software as a product from 9 December 2026;
  - the Cyber Resilience Act (vulnerability reporting since 11 September 2026, full requirements from 11 December 2027).

  This repository makes no claim of compliance with any of them.
- **TLS interop.** The c104 2.2.1 client cannot be used for TLS here: its bundled mbedtls 3.6 refuses to verify a server without a hostname (`-0x5D80`), and the released binding cannot set one yet. The TLS tests therefore send raw IEC 104 frames through Python's `ssl` (OpenSSL).

## License

MIT. Built by Michael Hiarley Silva Andrade, electrical engineering student at UFC, Brazil.
