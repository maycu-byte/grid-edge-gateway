# Rebound after a §14a reduction: 20 sites, 10 repetitions per case

Loads per site (feeder load ÷ 20), one-minute means from 16.5 h to 22 h; ± is the half-width of the 95% confidence interval (Student's t) over the repetitions. Rebound: how far the load after the first release rises above the same days without a reduction under the same controller (paired by weather and fleet). Shed: energy the vans and the heat pump wanted during the reduction but did not get.

## main (winter)

| Case | Peak after, kW | Peak after (15 min), kW | Steepest rise, kW/min | Rebound, kW | Energy pushed later, kWh | Shed, kWh | EV energy missing, kWh | Below comfort, K·h | Energy cost 16:30–22:00, € |
|---|---|---|---|---|---|---|---|---|---|
| no dimming | 35.8 ± 0.1 | 35.4 ± 0.1 | 0.5 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 173.88 ± 0.05 |
| step | 102.0 ± 0.1 | 101.7 ± 0.1 | 67.5 ± 0.1 | 77.3 ± 0.0 | 87.0 ± 0.0 | 156.1 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 150.61 ± 0.05 |
| ramp 5 min | 102.1 ± 0.1 | 101.7 ± 0.1 | 16.8 ± 0.1 | 77.3 ± 0.0 | 87.1 ± 0.0 | 156.1 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 150.41 ± 0.05 |
| ramp 5 min + wait ≤10 min | 102.1 ± 0.1 | 101.7 ± 0.1 | 8.6 ± 0.4 | 77.3 ± 0.0 | 87.0 ± 0.0 | 156.1 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 149.93 ± 0.02 |
| ramp 5 min + wait ≤30 min | 101.9 ± 0.1 | 100.2 ± 0.4 | 4.5 ± 0.5 | 77.3 ± 0.0 | 86.8 ± 0.1 | 156.1 ± 0.0 | 0.03 ± 0.02 | 0.00 ± 0.00 | 148.74 ± 0.13 |
| ramp 30 min | 102.1 ± 0.1 | 101.8 ± 0.1 | 3.2 ± 0.1 | 77.3 ± 0.0 | 87.0 ± 0.0 | 156.1 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 149.44 ± 0.05 |
| no dimming, planner | 46.0 ± 0.1 | 45.7 ± 0.1 | 1.1 ± 0.2 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.08 ± 0.01 | 31.15 ± 0.06 |
| planner + ramp 5 min | 45.1 ± 0.3 | 44.7 ± 0.4 | 9.5 ± 0.4 | 17.9 ± 0.3 | 3.2 ± 0.1 | 161.2 ± 0.1 | 0.00 ± 0.00 | 0.08 ± 0.01 | 33.80 ± 0.06 |

## mixed (winter, mixed fleets)

| Case | Peak after, kW | Peak after (15 min), kW | Steepest rise, kW/min | Rebound, kW | Energy pushed later, kWh | Shed, kWh | EV energy missing, kWh | Below comfort, K·h | Energy cost 16:30–22:00, € |
|---|---|---|---|---|---|---|---|---|---|
| no dimming | 43.6 ± 3.2 | 41.1 ± 3.1 | 0.5 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 163.40 ± 1.59 |
| step | 95.8 ± 1.4 | 93.4 ± 1.5 | 61.8 ± 1.4 | 55.7 ± 1.9 | 79.0 ± 5.1 | 142.7 ± 2.3 | 0.17 ± 0.11 | 0.00 ± 0.00 | 142.96 ± 1.50 |
| ramp 5 min | 95.3 ± 1.5 | 92.5 ± 1.6 | 16.2 ± 0.3 | 56.1 ± 1.9 | 78.9 ± 5.1 | 142.7 ± 2.3 | 0.26 ± 0.13 | 0.00 ± 0.00 | 142.73 ± 1.51 |
| ramp 5 min + wait ≤10 min | 92.8 ± 1.9 | 90.4 ± 1.8 | 7.8 ± 0.6 | 56.1 ± 1.9 | 78.6 ± 5.2 | 142.7 ± 2.3 | 0.52 ± 0.16 | 0.00 ± 0.00 | 141.94 ± 1.57 |
| ramp 5 min + wait ≤30 min | 87.5 ± 2.0 | 85.0 ± 2.5 | 4.3 ± 0.5 | 55.7 ± 2.7 | 76.5 ± 5.2 | 142.7 ± 2.3 | 1.46 ± 0.21 | 0.00 ± 0.00 | 139.99 ± 1.75 |
| ramp 30 min | 89.7 ± 2.1 | 87.6 ± 1.9 | 3.1 ± 0.1 | 57.0 ± 2.0 | 78.1 ± 5.2 | 142.7 ± 2.3 | 0.98 ± 0.19 | 0.00 ± 0.00 | 141.44 ± 1.55 |
| no dimming, planner | 39.7 ± 0.9 | 39.0 ± 0.7 | 2.6 ± 1.3 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.09 ± 0.01 | 30.10 ± 0.30 |
| planner + ramp 5 min | 39.5 ± 1.3 | 38.2 ± 1.5 | 6.2 ± 1.1 | 2.8 ± 0.7 | 0.8 ± 0.2 | 157.8 ± 1.4 | 0.00 ± 0.00 | 0.08 ± 0.01 | 32.13 ± 0.33 |

## spring (spring)

| Case | Peak after, kW | Peak after (15 min), kW | Steepest rise, kW/min | Rebound, kW | Energy pushed later, kWh | Shed, kWh | EV energy missing, kWh | Below comfort, K·h | Energy cost 16:30–22:00, € |
|---|---|---|---|---|---|---|---|---|---|
| no dimming | 30.5 ± 0.1 | 30.1 ± 0.1 | 0.5 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 37.50 ± 1.02 |
| ramp 5 min | 93.2 ± 0.4 | 87.1 ± 1.2 | 16.1 ± 0.2 | 63.2 ± 0.5 | 46.8 ± 3.1 | 118.9 ± 3.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 39.83 ± 1.26 |
| ramp 5 min + wait ≤30 min | 80.4 ± 2.0 | 77.3 ± 2.4 | 3.9 ± 0.4 | 61.3 ± 1.9 | 46.8 ± 3.1 | 118.9 ± 3.0 | 0.03 ± 0.02 | 0.00 ± 0.00 | 39.83 ± 1.25 |
| no dimming, planner | 18.2 ± 0.3 | 18.1 ± 0.3 | 6.6 ± 0.4 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.08 ± 0.01 | 17.76 ± 0.18 |
| planner + ramp 5 min | 28.1 ± 0.8 | 28.1 ± 0.8 | 18.1 ± 0.1 | 10.1 ± 0.8 | 9.7 ± 0.5 | 144.9 ± 0.4 | 0.00 ± 0.00 | 0.08 ± 0.01 | 17.88 ± 0.22 |

## staggered (winter)

| Case | Peak after, kW | Peak after (15 min), kW | Steepest rise, kW/min | Rebound, kW | Energy pushed later, kWh | Shed, kWh | EV energy missing, kWh | Below comfort, K·h | Energy cost 16:30–22:00, € |
|---|---|---|---|---|---|---|---|---|---|
| no dimming | 35.8 ± 0.1 | 35.4 ± 0.1 | 0.5 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 173.88 ± 0.05 |
| 1 group | 102.1 ± 0.1 | 101.7 ± 0.1 | 16.8 ± 0.1 | 77.3 ± 0.0 | 87.1 ± 0.0 | 156.1 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 150.41 ± 0.05 |
| 2 groups, 15 min apart | 102.1 ± 0.1 | 101.7 ± 0.1 | 8.4 ± 0.1 | 77.3 ± 0.0 | 87.0 ± 0.0 | 166.0 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 149.69 ± 0.05 |
| 4 groups, 15 min apart | 95.4 ± 0.2 | 90.1 ± 0.1 | 4.4 ± 0.1 | 70.9 ± 0.0 | 84.2 ± 0.0 | 185.8 ± 0.0 | 1.50 ± 0.00 | 0.00 ± 0.00 | 147.42 ± 0.05 |
| 4 groups + wait ≤10 min | 91.0 ± 0.5 | 85.2 ± 1.0 | 3.1 ± 0.3 | 66.2 ± 0.4 | 83.0 ± 0.1 | 185.8 ± 0.0 | 2.24 ± 0.09 | 0.00 ± 0.00 | 146.65 ± 0.04 |

## duration (winter)

| Case | Peak after, kW | Peak after (15 min), kW | Steepest rise, kW/min | Rebound, kW | Energy pushed later, kWh | Shed, kWh | EV energy missing, kWh | Below comfort, K·h | Energy cost 16:30–22:00, € |
|---|---|---|---|---|---|---|---|---|---|
| no dimming | 35.8 ± 0.1 | 35.4 ± 0.1 | 0.5 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 173.88 ± 0.05 |
| 1 h | 101.7 ± 0.1 | 101.3 ± 0.1 | 16.6 ± 0.2 | 66.2 ± 0.0 | 68.8 ± 0.0 | 77.5 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 159.88 ± 0.05 |
| 2 h | 102.1 ± 0.1 | 101.7 ± 0.1 | 16.8 ± 0.1 | 77.3 ± 0.0 | 87.1 ± 0.0 | 156.1 ± 0.0 | 0.00 ± 0.00 | 0.00 ± 0.00 | 150.41 ± 0.05 |
| 3 h | 80.9 ± 0.1 | 80.5 ± 0.1 | 15.9 ± 0.2 | 55.2 ± 0.0 | 68.1 ± 0.0 | 235.3 ± 0.0 | 8.81 ± 0.00 | 0.00 ± 0.00 | 141.24 ± 0.05 |

## notice (winter)

| Case | Peak after, kW | Peak after (15 min), kW | Steepest rise, kW/min | Rebound, kW | Energy pushed later, kWh | Shed, kWh | EV energy missing, kWh | Below comfort, K·h | Energy cost 16:30–22:00, € |
|---|---|---|---|---|---|---|---|---|---|
| no dimming, planner | 46.0 ± 0.1 | 45.7 ± 0.1 | 1.1 ± 0.2 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.0 ± 0.0 | 0.00 ± 0.00 | 0.08 ± 0.01 | 31.15 ± 0.06 |
| planner told in advance | 45.1 ± 0.3 | 44.7 ± 0.4 | 9.5 ± 0.4 | 17.9 ± 0.3 | 3.2 ± 0.1 | 161.2 ± 0.1 | 0.00 ± 0.00 | 0.08 ± 0.01 | 33.80 ± 0.06 |
| planner not told | 45.2 ± 0.3 | 45.1 ± 0.3 | 9.1 ± 0.3 | 17.8 ± 0.3 | 3.1 ± 0.0 | 161.1 ± 0.1 | 0.00 ± 0.00 | 0.05 ± 0.01 | 33.88 ± 0.07 |
