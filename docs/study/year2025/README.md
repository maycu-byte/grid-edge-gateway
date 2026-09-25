# 2025, evening by evening

Every evening of 2025 on its real data, for a feeder of 20 depots
(`crates/closedloop/src/bin/year.rs`):

```text
cargo run --release -p closedloop --bin year -- --sites 20 --out docs/study/year2025
```

Results: [`year.md`](year.md) (summary) and `year.json` (per day: the feeder
load in 5-minute steps per depot for the four cases, minutes over several
transformer sizes). The web demo reads a copy of `year.json` in
`web/year2025.json`.

## Data

`crates/devices/src/year2025_data.rs` holds 2025 in German local time,
365 days × 24 hours (hour 0 = 1 January 00:00 CET; summer time from
30 March to 26 October, the missing hour taking 03:00's values and the
repeated one its first pass).

| Series | Source | Licence |
|---|---|---|
| Day-ahead price, bidding zone DE-LU, €/MWh | Bundesnetzagentur \| SMARD.de, fetched through the Energy-Charts API (`api.energy-charts.info/price?bzn=DE-LU`). Hourly until 30 September 2025; from 1 October the mean of the four 15-minute products of each hour. | CC BY 4.0 |
| Air temperature at 2 m, °C | Open-Meteo historical weather archive (ERA5 reanalysis, Copernicus Climate Change Service / ECMWF), 48.78 N 9.18 E (Stuttgart), hourly | CC BY 4.0 |
| Global horizontal irradiance, W/m² | same, `shortwave_radiation`, mean over the hour | CC BY 4.0 |

Checks: the series reproduce the two study days exactly — 583.40 €/MWh on
20 January at 17:00 and −114.57 €/MWh on 6 April at 14:00
(`climate::tests::a_real_day_matches_the_two_study_days`).

## Model on a real day

- PV output, fraction of installed power = irradiance / 1000 W/m² × 0.85
  (performance ratio of a flat rooftop system).
- Outdoor temperature and prices come from the series; the forecaster keeps
  its clear-sky model (sun path at Stuttgart) and the cloudiness climatology
  of the half-year, and learns the day's cloudiness from the PV it sees.
- The vans, the building and the other loads are the modelled ones of the
  depot; every depot gets its own van timetable (`mixed`), all share the
  weather.
