# The planning layer: MPC on a §14a site

The real-time controller keeps the site inside the rules second by second. It cannot see ahead. It does not know that power will cost 583 €/MWh at 17:00. It does not know that the van on charger 2 leaves at 20:30, that the DSO will dim the site at 17:30, or that the building could have been warmed at noon for less. The planner does. Every 15 minutes, and whenever a car plugs in, it solves an optimisation over the next 24 hours and hands the result to the real-time layer as guidance.

This page describes the formulation, the design choices behind it, and a Monte Carlo study of what the planner is worth. The study compares plain rules with five MPC variants, over 30 random-weather days in spring and in winter, on real German day-ahead prices.

## Two layers

```
             every 15 min + on arrival                    every second
 forecasts ─► planner (convex QP, 96 × 15 min) ─guidance─► real-time layer ─► setpoints ─► devices
 prices       grid exchange, per-car power,                 §14a floor, feed-in limits,
 requests     heat-pump power, SoC and                      6–32 A in whole amps,
              indoor-temperature trajectories               departure guard, fallbacks
```

The plan is advice. The real-time layer applies every hard rule after it:

- **Battery.** It holds the planned grid exchange, so it absorbs forecast errors: an affine recourse with gain 1.
- **Chargers.** Each charger is capped at its planned current. A planned power under half the 6 A minimum means "not now". A car whose slack falls under 15 minutes charges at full power whatever the plan says. Slack is time to departure minus time to charge at the car's own maximum current, as ISO 15118 reports it.
- **Heat pump.** It gets the planned power as an external request (SG Ready / EEBUS style) and keeps its own comfort guard.
- **Floor and limits.** The consumption floor, feed-in limits, device minimums and failure fallbacks are the ones described in the [README](../README.md). They never depend on the plan being right.

A property test feeds 10,000 random plans into a dimmed controller and checks that the floor, the current rules and the heat-pump cap still hold.

## Formulation

Horizon of 96 steps of 15 minutes. The variables at each step `k` are:

- grid import g⁺ and export g⁻;
- PV used;
- battery charge c and discharge d, and stored energy E;
- heat-pump power hp and indoor temperature T;
- the power to each car i, evᵢ;
- slacks for comfort, the SoC band and the envelope.

The constraints are:

```text
balance     g⁺ₖ − g⁻ₖ + pvₖ − Σᵢ evᵢₖ − hpₖ − cₖ + dₖ = baseₖ
battery     Eₖ₊₁ = Eₖ + η·dt·cₖ − dt/η·dₖ,             E_min + tightening ≤ Eₖ ≤ E_max
building    Tₖ₊₁ = a·Tₖ + b·COPₖ·hpₖ + dt/C·(gainsₖ + UA·T_outₖ),  a = 1 − dt·UA/C
EV i        Σₖ dt·evᵢₖ + uᵢ ≥ energy requested before departure,   evᵢₖ = 0 after it
dimming     Σ evᵢₖ + hpₖ + cₖ − dₖ ≤ floor + max(0, PV_lowₖ − base_highₖ)   (where a dimming is expected)
peak        g⁺ₖ ≤ P̂,   P̂ ≥ peak already billed this period
```

The objective, in euros, is the sum of:

| Term | Value in the study | Why |
|---|---|---|
| Energy | import at day-ahead + 0.12 €/kWh; export at max(0, day-ahead) | A dynamic tariff (German suppliers must offer one, §41a EnWG). The adder stands for grid fees, levies and taxes. Exported PV earns nothing in negative hours (§51 EEG). |
| Battery ageing | 0.03 €/kWh in or out, plus 0.01 €/kWh·h outside 20–80% SoC | 300 €/kWh over 6,000 cycles at 80% depth, per kWh of throughput; the band term penalises time spent full or empty, where cells age faster. |
| Demand charge | 100 €/kW·a on P̂, the plan carrying 24/8760 of it | The *Leistungspreis* of metered commercial customers, billed on the year's or month's highest quarter-hour. Only the part above the peak already billed costs anything. |
| Discomfort | 20 €/K·h below the comfort floor or above 23 °C | High enough that the plan never trades comfort for a few cents of energy. |
| Energy a car leaves without | 1 €/kWh | Keeps the problem feasible when a request cannot be met, and reports how much is missing. |
| Smoothing | 10⁻⁴ €/kW² on battery power changes | Removes a flat optimum's chatter; the only quadratic term. |
| Terminal value | 90% of the mean import price × round-trip efficiency, per kWh left in the battery | Stops the plan from emptying the battery at the end of every horizon. |

The problem is solved by [Clarabel](https://github.com/oxfordcontrol/Clarabel.rs), an interior-point solver in pure Rust. With four cars it has about 1,550 variables. It solves in about 16 ms and 20 iterations on one laptop core, with or without the demand charge. The same code runs in the browser, compiled to WebAssembly. Solve times in the study tables are higher (mean 40–49 ms) because twelve runs share the CPU.

### Why a convex QP and not a MILP

Binary variables buy exactness in three places:

- **Import and export at once.** This can only pay when the import price drops below the export price. Here the import price is floored at the export price. With the 0.12 €/kWh adder, the floor binds only below −120 €/MWh, which neither study day reaches.
- **Minimum powers.** These are the 6 A per car and the heat pump's minimum modulation. They are left to the real-time layer: a plan under half of 6 A means off, and the heat pump cycles.
- **Charge and discharge at once.** This is never optimal when both directions lose energy and cost ageing.

What remains is convex. That gives a unique optimum, a certificate when the problem is infeasible, a predictable solve time, and a route to differentiable MPC, because the solution can be differentiated through its optimality conditions. A MILP gives none of these.

## Forecast errors: deterministic, chance-constrained, robust

The forecaster uses only what a real EMS would have:

- the site's clear-sky model;
- the season's climatology of cloudiness (mean, standard deviation, worst case);
- a nowcast of today's cloudiness, learned from the PV the inverters report as available;
- the depot's usual load profile, with 1.5 kW of standard deviation;
- day-ahead prices, which are known a day ahead.

Uncertainty enters only where a shortfall would hurt:

- **PV counted on during a dimming.** This is the expected PV, the expected PV minus z·σ (chance constraint, ε = 5%, z = 1.645), or the worst-case day (robust).
- **The lower edge of the battery's SoC envelope, in and just before an expected dimming.** It is tightened by the forecast error the battery may have absorbed by then (z·σ or worst case).

Outside a dimming, a shortfall is simply bought from the grid at the price of the moment. That is a cost risk the expected-value objective already weighs. Tightening there would make the battery charge early and expensively to insure against something harmless. The upper edge is not tightened either: a battery fuller than planned means more export or curtailment, not a comfort or compliance problem.

Forecast errors are summed linearly, not in quadrature, because a cloudy day is cloudy all day and the errors of neighbouring hours are strongly correlated. They are summed only over the time the next plans need to correct a deviation by buying from the grid (8 steps, 2 h), plus the whole dimming, when buying for the battery is not allowed. A first version that summed errors over the full 24 hours asked for more reserve than the battery holds. In that version the chance-constrained plan cost more than plain rules.

## The study

The site is the depot in Germany:

- 120 kWp of PV;
- four 22 kW chargers;
- a 14 kW heat pump in a building of UA 1.2 kW/K and C 18 kWh/K;
- a 100 kWh / 50 kW battery;
- seven cars a day (six vans and a visitor) asking for 260 kWh in total, three of them overnight.

Two real days of day-ahead prices (bidding zone DE-LU, SMARD) are used:

- **Spring (6 April 2025).** Down to −115 €/MWh at 14:00; 3–15 °C outside.
- **Winter (20 January 2025).** Up to 583 €/MWh at 17:00; −4 to 3 °C outside.

Each of 30 seeds draws that day's cloudiness from the season's climatology. Every run lasts 25.5 hours from 06:00, so the overnight vans leave inside it. The DSO dims from 17:30 to 19:30 (preventive control is capped at 2 hours a day until 2028, BK6-22-300 10.5). The control cycle is 5 s, and the plan is updated every 15 min. Every quantity is measured on the simulation's physical state, not on what the controller believes. Differences are paired: each run is compared with the rule-based run on the same weather.

The strategies:

| | |
|---|---|
| `rules` | The real-time layer alone: the battery maximises self-consumption, cars charge at full power on arrival (least slack first when limited), the heat pump follows its own thermostat (21 °C by day, 18 °C at night). |
| `mpc-no-peak` | MPC without the demand charge. |
| `mpc-blind` | MPC that does not know the dimming window in advance (it learns of it when the command arrives). |
| `mpc` | MPC, deterministic. |
| `mpc-cc` | MPC with chance constraints (ε = 5%). |
| `mpc-robust` | MPC against the worst-case cloudiness. |

### Results: evening dimming (17:30–19:30)

Means over 30 days, ± half-width of the 95% confidence interval. Full tables: [docs/study/study.md](study/study.md), every run in [study.json](study/study.json).

| | Spring: total € | Δ vs rules € | Peak kW | Winter: total € | Δ vs rules € | Peak kW |
|---|---|---|---|---|---|---|
| rules | 126.6 ± 5.8 | — | 89 | 357.9 ± 6.2 | — | 110 |
| mpc-no-peak | 111.0 ± 3.9 | −15.6 ± 2.4 | 93 | 313.5 ± 6.3 | −44.4 ± 0.2 | 118 |
| mpc-blind | 101.0 ± 3.9 | −25.6 ± 2.2 | 34 | 298.3 ± 8.5 | −59.6 ± 2.4 | 57 |
| **mpc** | **101.0 ± 3.9** | **−25.6 ± 2.2** | **34** | **298.2 ± 8.6** | **−59.7 ± 2.4** | **57** |
| mpc-cc | 101.0 ± 3.8 | −25.6 ± 2.2 | 34 | 298.4 ± 8.7 | −59.5 ± 2.6 | 57 |
| mpc-robust | 101.0 ± 3.9 | −25.6 ± 2.2 | 34 | 298.6 ± 8.7 | −59.3 ± 2.6 | 57 |

In every run of every strategy, every car left with the energy it asked for, and no plan failed. The dimming floor is discussed below.

### Sensitivity: afternoon dimming (13:00–15:00)

The same study, with the DSO dimming in the early afternoon, when PV is high and uncertain. Full tables: [docs/study/afternoon/study.md](study/afternoon/study.md).

| | Spring: total € | Δ vs rules € | Peak kW | Winter: total € | Δ vs rules € | Peak kW |
|---|---|---|---|---|---|---|
| rules | 123.8 ± 5.5 | — | 85 | 381.7 ± 6.1 | — | 110 |
| mpc-no-peak | 114.3 ± 4.5 | −9.5 ± 1.8 | 99 | 315.2 ± 6.5 | −66.5 ± 0.4 | 121 |
| mpc-blind | 104.4 ± 5.1 | −19.5 ± 1.7 | 35 | 304.8 ± 8.4 | −76.9 ± 2.3 | 57 |
| **mpc** | **105.0 ± 5.3** | **−18.8 ± 1.8** | **34** | **305.0 ± 8.3** | **−76.6 ± 2.3** | **59** |
| mpc-cc | 110.2 ± 5.1 | −13.7 ± 1.8 | 35 | 306.3 ± 7.6 | −75.4 ± 1.6 | 58 |
| mpc-robust | 111.1 ± 4.7 | −12.7 ± 1.7 | 36 | 306.1 ± 7.8 | −75.6 ± 1.7 | 59 |

The rules-based site costs 24 € more in winter when the dimming comes in the afternoon than in the evening. In the evening the DSO's dimming happens to cut its car charging during the price peak (about 300–580 €/MWh between 17:00 and 20:00); in the afternoon nothing stops it from charging at full power into that peak.

## Findings

**1. The value is in prices, the peak and the building. It is not in handling uncertainty.**

In winter the MPC saves 59.7 € a day against rules (−17%):

- **Energy (51.4 €).** It moves heat-pump and charging energy out of the evening price peak (400–583 €/MWh from 16:00 to 19:00). It also runs the battery as a price arbitrage: about 1.3 cycles a day, where rules barely use it without PV surplus.
- **Peak charge (15.3 €).**
- **Minus 7.0 € of extra battery ageing.**

In spring the saving is 25.6 € (−20%): 11.5 € of energy and 16.0 € of peak charge, minus 1.9 € of ageing.

**2. Without a demand charge in the objective, an MPC raises the peak.**

The price-only plan (`mpc-no-peak`) piles charging into the cheapest hours. It raises the site's highest quarter-hour from 110 to 118 kW in winter (89 to 93 kW in spring). With the demand charge, the plan gives up 5–7 € of energy savings and brings the peak down to 57 kW (winter) and 34 kW (spring). For a commercial site in Germany, whose grid fees are largely a *Leistungspreis*, a price-only optimiser can lose money.

**3. Chance-constrained and robust planning did not pay in money.**

With the evening dimming there is little sun to be uncertain about, and the three variants are indistinguishable. With the afternoon dimming, the reserve they keep costs 5.1 € (chance-constrained) and 6.1 € (robust) a day in spring, and about 1 € in winter. It buys no measurable saving and no comfort, and every car was served either way. What it does buy is margin on the dimming floor (finding 5). Three things make the reserve unnecessary for money on this site:

- **Recourse.** The real-time layer corrects forecast errors within seconds, with the battery.
- **The floor.** §14a guarantees Pmin,14a even while dimmed, so a shortfall only slows the loads.
- **The grid.** Outside the dimming it covers any deficit at the price of the moment.

The reserve would pay where a shortfall cannot be bought back:

- a schedule the site has committed to, such as a VPP's or a balancing group's *Fahrplan* (deviations cost imbalance prices);
- a limit on the whole connection rather than on the controllable devices;
- islanded operation.

**4. Knowing the dimming window in advance mattered little for cost.**

A peak-aware plan spreads its load, which leaves the real-time layer enough slack to handle a dimming it did not expect. The blind plan cost the same (−19.5 vs −18.8 € in spring afternoons, within the confidence interval). It did spend more seconds above the floor (see below), because it had scheduled loads that the dimming then had to cut.

**5. The floor held within seconds, whatever the plan.**

The check allows 0.5 kW of tolerance and starts 60 s after a dimming begins, the time the devices get to follow a new setpoint. Across the 720 runs, the controllable devices never drew more than 2.1 kW above the floor, and only in short bursts (one or two 5 s control cycles in the runs we traced). They happened when the budget was binding and the sun or the base load moved between two cycles.

On average, per two-hour dimming, the time above the floor was:

| Strategy | Spring afternoon | Winter afternoon | Evenings (both seasons) |
|---|---|---|---|
| `mpc-blind` | 36 s | 2 s | 0–2 s |
| `mpc-no-peak` | 25 s | 0 s | 0–2 s |
| `mpc` | 7 s | 0 s | 0–1 s |
| `rules` | 0 s (its cars had finished charging by then) | 0 s | 0–3 s |
| `mpc-cc`, `mpc-robust` | 0 s | 0 s | 0 s |

The chance-constrained and robust plans count on less PV during a dimming, so they had none. In energy, that is on average at most 0.005 kWh per dimming.

Two changes brought these bursts down:

- **Holding the surplus.** A first version of the real-time layer counted on the PV surplus it had just measured. The version tested here counts on the lowest surplus of the last 30 s (`surplus_hold_s`). That cut the time above the floor by about 60%.
- **A faster loop.** The gateway runs its loop every second, not every 5 s as the study does.

A larger `margin_kw` would remove the rest, at the cost of some charging power in every dimming.

## Notes on common claims

These came up while designing the planner. Each was checked against the sources linked in the [README](../README.md).

- **§14a limits the controllable devices' draw from the grid, not the site's import.** Pmin,14a is a floor for the heat pumps, chargers, batteries and air conditioners together. The rest of the site's load is never limited. PV surplus and battery discharge come on top.
- **§14a is about consumption.** It obliges the DSO to connect controllable consumers without delay, in exchange for the right to dim them. It gives no rights over generation, which is handled elsewhere: EEG §9, redispatch under §13a EnWG, and the Solarspitzengesetz.
- **Nothing trips at 4.2 kW.** 4.2 kW is the minimum each controllable device keeps while dimmed (40% of the rating for a heat pump above 11 kW, and scaled by a simultaneity factor when several devices sit behind an EMS). An EMS may split the site's total freely.
- **The Austrian peak cap (Spitzenkappung) is in the ElWG, not the EAG.**
- **Frequency is the TSOs' task, not the DSOs'.** The four German TSOs, APG in Austria and Swissgrid in Switzerland handle it. §14a and its neighbours are about local grid congestion.
- **An SoC-band ageing term must penalise time outside the band, not reward it.** Cells age faster when kept full (calendar ageing) or deeply cycled near empty.
- **Binaries are not needed for this problem** (see above), and keeping it convex is what makes a differentiable MPC possible.

## What was not done

- **Deep reinforcement learning.** A policy trained in this simulator could take the planner's place in the same two-layer architecture, with the real-time layer still guaranteeing the rules. It would be a fair comparison, but it needs many simulated years to train and gives no guarantee of its own. It is future work.
- **Differentiable MPC.** The convex QP allows gradients of the closed-loop cost with respect to parameters such as the comfort weight, the terminal value or a forecast bias. Learning them from operation is future work.
- **Vehicle-to-grid and vehicle-to-home.** Cars only charge.

## Limits

- **The building is the planner's own model.** The simulation uses the same first-order thermal model and parameters as the planner, so the only mismatch comes from forecasts. A real building needs its UA and C identified from data first, and the gains would shrink.
- **The rules baseline is plain.** All of its discomfort (1.8 K·h a day) is the warm-up after the night setback, 06:00–07:30 on the second morning. A thermostat with optimum start would avoid most of it. The MPC's comfort advantage says more about the baseline than about MPC. A price-aware rule set (charge below a price threshold) would also narrow the cost gap.
- **Two real price days.** Each season has one day of prices, repeated. The weather varies across seeds, the prices do not. The savings depend strongly on the price spread, and 20 January 2025 had an unusually high evening peak.
- **The control cycle is 5 s in the study**, to keep 720 runs fast; the gateway's is 1 s.
- **The demand charge is a proxy.** Real bills use the highest quarter-hour of the month or year. The study charges each run its share of a year on its own peak, as if every day of the billing period looked like it. In operation, the planner takes the billing period's running peak and the full price instead (`DemandCharge::peak_so_far_kw`).
- **Not yet in the gateway binary.** See the README.
