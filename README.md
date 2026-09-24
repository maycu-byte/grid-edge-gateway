# Grid Edge Gateway

**A site controller in Rust between a German grid operator and a prosumer site: IEC 60870-5-104 towards the DSO, SunSpec Modbus towards the devices, §14a EnWG dimming and feed-in limits in between.**

**[Open the live demo →](https://maycu-byte.github.io/grid-edge-gateway/)** The demo runs this code in your browser, compiled to WebAssembly. You play the grid operator: send the commands and watch the site respond, frame by frame.

![A simulated day: feed-in limit at noon, §14a dimming in the evening](docs/screenshot-day.png)

## The problem

German distribution grids now have to control what happens behind the meter:

- **§14a EnWG** (BNetzA ruling BK6-22-300, in force since 2024) lets the DSO dim heat pumps, wallboxes and batteries when the local grid is overloaded. The site must still get a guaranteed minimum power, *Pmin,14a*, and the site's energy management system (EMS) decides how to share it.
- **Feed-in limits:** PV plants from 100 kW must be remotely controllable by the DSO (EEG §9). The DSO sends active-power setpoints (100 / 60 / 30 / 0 %) over telecontrol, often **IEC 60870-5-104**.

The control box in between has to speak the DSO's protocol on one side and the devices' protocols on the other, apply the regulatory rules correctly, and stay safe when a link, a device or the box itself fails. This project builds that box.

## The demo site

A logistics depot with **120 kWp** rooftop PV (two 60 kW inverters), **four 22 kW chargers** for delivery vans and a **14 kW heat pump**.

```
DSO control centre ──IEC 104 / TLS──► grid-edge-gateway ──Modbus TCP / SunSpec──► inverters, meter, chargers, heat pump
      (SCADA)                           (this repository)                         (site-sim, or real devices)
```

## What the gateway does

| | |
|---|---|
| **§14a dimming via EMS** | Pmin,14a is computed exactly as in BK6-22-300 Anlage 1, 4.5.2: `max(0.4·ΣP_WP, 0.4·ΣP_Klima) + (n−1)·GZF·4.2 kW` when a heat pump is above 11 kW, else `4.2 kW + (n−1)·GZF·4.2 kW`. For the depot (n = 5, GZF 0.65): **16.52 kW**. The limit applies to power *drawn from the grid*, so PV surplus is added on top. |
| **Allocation** | Each heat pump first gets 40% of its rating. Then as many cars as fit get the IEC 61851 minimum of 6 A, least-charged first. What is left tops up the heat pump, then the cars in whole amps. Every value is rounded *down*: when the exact value is impossible, the regulation asks for the next lower one. Cars are only rotated after a 5-minute dwell time. |
| **Gradual release** | When the dimming ends, power returns linearly over 5 minutes instead of in one step (BK6-22-300, 4.3). |
| **Feed-in limit** | Applied at the grid connection point: export ≤ limit × 120 kW, and PV may produce more while the site consumes it. A plant-output mode is configurable. |
| **Fail-safe design** | See below. |

## IEC 104 point list (common address 1)

| IOA | Type | Direction | Meaning |
|---|---|---|---|
| 5001 | C_SC_NA_1 | DSO → site | §14a dimming ON / OFF |
| 5002 | C_SE_NC_1 | DSO → site | Feed-in limit, % of installed PV (0–100; other values get a negative confirmation) |
| 1001 | M_ME_TF_1 | site → DSO | Active power at the grid connection, kW (+ import) |
| 1002 | M_ME_TF_1 | site → DSO | PV active power, kW |
| 1003 | M_ME_TF_1 | site → DSO | Controllable devices (steuVE), kW |
| 1004 | M_ME_TF_1 | site → DSO | steuVE power drawn from the grid, kW: the quantity §14a limits |
| 1005 | M_ME_TF_1 | site → DSO | Pmin,14a of this site, kW |
| 1006 | M_ME_TF_1 | site → DSO | Feed-in limit in force, % (feedback of 5002) |
| 1007 | M_ME_TF_1 | site → DSO | PV limit sent to the inverters, % |
| 2001 | M_SP_TB_1 | site → DSO | §14a dimming active (feedback of 5001) |
| 2002 | M_SP_TB_1 | site → DSO | Gradual release in progress |
| 2003 | M_SP_TB_1 | site → DSO | Meter fallback (grid measurement lost) |
| 2004 | M_SP_TB_1 | site → DSO | Field device fault |

Commands support direct execute and select-before-operate. Unknown addresses, types, causes and common addresses get the negative confirmations the standard defines (causes 44–47). Measurements are sent spontaneously outside a deadband and all together in a general interrogation.

![Evening dimming: chargers at 7 A, heat pump at its minimum, and the IEC 104 frames that caused it](docs/screenshot-dimming.png)

## Fail-safe design

| Failure | What happens |
|---|---|
| Gateway crashes or hangs | Each charger's own watchdog (armed by the gateway at start) drops it to **6 A = 4.14 kW** after 30 s, below 4.2 kW on its own. Inverter limits are written with **no revert timeout** (SunSpec `WMaxLimPct_RvrtTms = 0`), so the last DSO limit stays in force. |
| Gateway restarts | DSO commands are persisted (write-then-rename), so a dimming that was active before a power cut is active again after it. |
| Grid meter lost | PV surplus can no longer be seen, so the §14a budget shrinks to Pmin. The feed-in limit falls back to plant-output mode. Point 2003 goes ON. |
| A charger or the heat pump stops answering | It is assumed to draw its failsafe current (charger) or rated power (heat pump), and the budget for the others shrinks accordingly. |
| DSO link lost | The last commands stay in force until the DSO reconnects and changes them. |
| Someone else reaches port 2404 | With TLS on, the station accepts only client certificates signed by the DSO's CA (TLS 1.2/1.3, in the spirit of IEC 62351-3). |

## How it is built

| Crate | What it is |
|---|---|
| [`iec104`](crates/iec104) | IEC 60870-5-104 from scratch, with no dependencies: APDU framing, the ASDUs above plus clock sync and interrogation, CP56Time2a, and the controlled-station link layer (k/w windows, t1/t2/t3 timers, 15-bit sequence numbers) as a **sans-IO state machine**, so every timing rule is unit-tested without sleeping. An optional tokio driver runs it over TCP or TLS. |
| [`control`](crates/control) | The rules above as pure functions and a small state machine. The same code runs in the gateway, in the tests and in the browser. |
| [`devices`](crates/devices) | SunSpec register layouts (models 1, 103, 120, 123, 203), a typical wallbox map with failsafe current and heartbeat, and a deterministic physics simulation of the depot. |
| [`gateway`](crates/gateway) | The binary: one supervised task per Modbus device (SunSpec discovery by walking the model chain, reconnect, staleness detection), a 1 s control loop, the IEC 104 station, TLS (rustls), and a read-only JSON/WebSocket API. |
| [`site-sim`](crates/site-sim) | Every device of the depot as its own Modbus TCP server, with an HTTP endpoint to take devices offline. |
| [`web-demo`](crates/web-demo) | The browser build: simulation + controller + IEC 104 encoder in WebAssembly. The controller reads and writes the simulated devices through their register maps, like the gateway does over Modbus TCP. |

## Testing

- **50 Rust tests.** They cover protocol frames checked against reference octets, every link-layer timer and window, sequence-number wrap-around, the Pmin formula for several device mixes, allocation scenarios, and a property test over 20,000 random site states: while dimmed, the granted power never exceeds Pmin + PV surplus, every charger current is 0 or 6–32 A in whole amps, and no heat pump runs below its minimum modulation.
- **10 interoperability tests** ([`interop/`](interop/test_interop.py)). They start the real simulator and gateway and drive them with [c104](https://github.com/Fraunhofer-FIT-DIEN/iec104-python), a Python binding of lib60870, acting as the DSO control centre. They cover general interrogation, §14a compliance within seconds, gradual release, negative confirmations, meter loss, persistence across a gateway restart, and TLS acceptance and rejection.
- CI runs `fmt`, `clippy -D warnings`, all tests and the interop suite on every push, then builds the WebAssembly demo and deploys it to GitHub Pages.

## Run it locally

Needs Rust (stable) and, for the interop tests, Python 3.10+.

```sh
cargo build
./target/debug/site-sim --start 17:30          # the depot, Modbus TCP on :5020–5027
./target/debug/gateway gateway.toml            # IEC 104 on :2404, API on :8080
```

Then act as the DSO with any IEC 104 master (address 1, IOA 5001 / 5002), or run the tests:

```sh
pip install -r interop/requirements.txt
sh certs/gen-demo.sh                           # throw-away PKI for the TLS tests
python -m pytest -v interop
```

To enable TLS, uncomment `[iec104.tls]` in `gateway.toml`. For the browser demo, run `web/build.sh`, which needs the `wasm32-unknown-unknown` target and `wasm-bindgen-cli` 0.2.128.

## Limits and honest notes

- **Simulated devices.** The wallbox and heat-pump register maps follow common patterns but are not a specific product's map. SunSpec layouts follow the published models.
- **Signal path.** In German households the §14a signal usually travels through the smart meter gateway (CLS channel) to an FNN control box or via EEBUS. IEC 104 is the standard telecontrol path for larger plants (from 100 kW). This project uses IEC 104 for both commands to keep one DSO interface; the control logic does not depend on the transport.
- **Protocol scope.** The IEC 104 stack implements the subset a controlled station of this kind needs, not the full companion standard (no file transfer, no redundancy groups). It is not certified; it is tested against lib60870.
- **TLS interop.** The c104 2.2.1 client cannot be used for TLS here: its bundled mbedtls 3.6 refuses to verify a server without a hostname (`-0x5D80`), and the released binding cannot set one yet. The TLS tests therefore send raw IEC 104 frames through Python's `ssl` (OpenSSL).
- **Pmin formula** as published by BNetzA in BK6-22-300 (2023); DSOs' technical connection rules add details not modelled here.

## License

MIT. Built by Michael Hiarley Silva Andrade, electrical engineering student at UFC, Brazil.
