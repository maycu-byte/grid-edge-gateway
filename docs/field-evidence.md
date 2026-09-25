# What the public data says about §14a dimming

Checked on 25 September 2026. The question: how often do German grid operators actually dim heat pumps, chargers and batteries under §14a EnWG, and where? The gateway's evening study assumes a dimming whenever a feeder would overload. This page records what can be verified instead.

## The devices exist, in large numbers

The Bundesnetzagentur's monitoring report 2025 ([PDF](https://data.bundesnetzagentur.de/Bundesnetzagentur/SharedDocs/Mediathek/Monitoringberichte/MonitoringberichtEnergie2025.pdf), p. 115–116, table 28) counts the market locations that fall under the new §14a rules, installed since 1 January 2024, as of 31 December 2024:

| Grid-fee module | Total | Charging points | Heat pumps | Batteries | Air conditioning |
|---|---|---|---|---|---|
| Module 1 (flat reduction) | 239,825 | 62,104 | 62,408 | 128,475 | 552 |
| Module 2 (separate meter) | 25,049 | 2,488 | 22,012 | 867 | 19 |
| **Total** | **264,874** | **64,592** | **84,420** | **129,342** | **571** |

Another 1,918,573 devices had joined the older, voluntary scheme before 2024 (mostly night storage heaters and heat pumps).

So in its first year the new rule already covered a quarter of a million sites. Every one of them must accept dimming to Pmin,14a, and its operator must keep the proof (BK6-22-300, Anlage 1, 7.2–7.3).

## The dimmings are not recorded yet

Since 1 March 2025 every distribution system operator has to publish its §14a control actions on VNBdigital by the 15th of the following month ([BDEW format, 30 Sep 2024](https://www.bundesnetzagentur.de/DE/Beschlusskammern/1_GZ/BK6-GZ/2022/BK6-22-300/Mitteilung/Format__f%C3%BCr_die_Umseetzung_der_Ver%C3%B6ffentlichungspflichten.pdf?__blob=publicationFile&v=1)). The format has, per month:

- the grid area (an ID given by the operator) and the postcodes it covers;
- the kind of control: grid-oriented (Anlage 1, 4.) or preventive (10.5.);
- the number of controllable devices in the area;
- the hours with an intervention in the month;
- the intensity: the share of the devices' installed power that was not available because of the control.

The public page on VNBdigital ([vnbdigital.de/service/controlMeasures](https://www.vnbdigital.de/service/controlMeasures)) states, as of 25 September 2026:

> "Derzeit sind noch keine Steuerungsmaßnahmen in VNBdigital erfasst, die obige Suche liefert entsprechend noch keine Ergebnisse."
>
> (No control measures are recorded in VNBdigital yet; the search above returns no results.)

An operator that does control shows a service "Netzdienliche Steuerung" on its VNBdigital profile.

## What this means for the gateway

- **The obligation is real and large.** A quarter of a million sites must be dimmable, and each needs the proof the gateway writes.
- **The dimming itself has not been observed in public data.** Either no operator has dimmed yet, or none has published it. Operators are only now rolling out the control boxes and smart meter gateways that make dimming possible.
- **So the evening problem in [docs/study/year2025](study/year2025/README.md) is a forecast, not a measurement.** It shows what would happen on a feeder of 20 depots if operators dimmed on every day the transformer would overload. It does not show that they do.
- **What would test it.** Once VNBdigital lists control actions, the hours per month and area give the real frequency of dimming. The study's assumption can then be replaced by it, area by area. The gateway itself does not depend on that: it has to follow a dimming whenever one comes.

## How the data can be collected

VNBdigital serves its data through a GraphQL endpoint (`https://www.vnbdigital.de/gateway/graphql`, used by the open client [vnbdigital-client](https://github.com/the78mole/vnbdigital-client) for operator lookups). The server did not accept connections from the networks this study ran on (Brazil and a US cloud), while a reader service in Europe could load the public pages. A collector for the control actions should therefore run from a German or EU network, once there is something to collect.
