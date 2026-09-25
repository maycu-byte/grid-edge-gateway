# 2025, evening by evening

20 depots on one feeder, each with its own van timetable, simulated on every evening from 2025-01-01 to 2025-12-31 with that day's real day-ahead prices (SMARD, DE-LU) and measured weather (Open-Meteo ERA5, Stuttgart). Four cases per day: rules and planner, each without a reduction and with a §14a reduction from 17:30 to 19:30 ended by the 5-minute ramp. A grid operator is assumed to reduce only on days the feeder would otherwise go over its transformer between 16:30 and 22:00. *Rebound overload* = minutes over the transformer after 19:30 with the reduction, minus the same minutes without it.

## How often, by transformer size

| Transformer per depot | Days the feeder would overload (rules) | … of which the reduction's rebound overloads it again (rules) | … (planner) | Rebound overload, minutes in the year (rules) | … (planner) | Days the planner alone keeps within the transformer |
|---|---|---|---|---|---|---|
| 60 kW | 204 | 204 | 0 | 12338 | 0 | 204 of 204 |
| 70 kW | 193 | 193 | 0 | 7231 | 0 | 193 of 193 |
| 80 kW | 179 | 178 | 0 | 4268 | 0 | 179 of 179 |
| 90 kW | 148 | 112 | 0 | 1355 | 0 | 148 of 148 |
| 100 kW | 3 | 1 | 0 | 2 | 0 | 3 of 3 |
| 110 kW | 0 | 0 | 0 | 0 | 0 | 0 of 0 |
| 120 kW | 0 | 0 | 0 | 0 | 0 | 0 of 0 |

## By month, 90 kW per depot

| Month | Mean temperature, °C | PV, kWh/kWp | Evening price, €/MWh | Days over (rules) | Rebound overload days (rules) | (planner) | Mean peak after release, kW/depot (rules → planner) |
|---|---|---|---|---|---|---|---|
| Jan | 2.7 | 0.9 | 152 | 31 | 27 | 0 | 95 → 37 |
| Feb | 2.5 | 1.6 | 166 | 27 | 24 | 0 | 94 → 36 |
| Mar | 6.6 | 2.7 | 149 | 12 | 10 | 0 | 86 → 33 |
| Apr | 10.9 | 4.3 | 105 | 0 | 0 | 0 | 53 → 21 |
| May | 14.6 | 4.9 | 85 | 0 | 0 | 0 | 42 → 15 |
| Jun | 20.8 | 5.7 | 76 | 0 | 0 | 0 | 34 → 14 |
| Jul | 19.6 | 4.8 | 99 | 0 | 0 | 0 | 36 → 17 |
| Aug | 19.5 | 4.5 | 102 | 0 | 0 | 0 | 46 → 18 |
| Sep | 14.9 | 2.7 | 143 | 2 | 1 | 0 | 73 → 24 |
| Oct | 10.5 | 1.8 | 133 | 15 | 3 | 0 | 87 → 31 |
| Nov | 5.4 | 1.1 | 135 | 30 | 20 | 0 | 92 → 36 |
| Dec | 3.1 | 0.9 | 112 | 31 | 27 | 0 | 94 → 37 |

Energy the vans left without over the year, whole feeder: rules 0 kWh without and 357 kWh with the daily reduction; planner 0 and 118 kWh.
