# eebus-bridge

The EEBUS side of the gateway. Behind an FNN control box, a German site with an energy manager receives §14a limits over EEBUS, in the use case **LPC** (Limitation of Power Consumption). The bridge is that energy manager's EEBUS end: the **Controllable System** of LPC.

It is written in Go on [eebus-go](https://github.com/enbility/eebus-go) v0.7.0 (MIT), the EEBUS stack that the open-source energy manager evcc also uses.

```
FNN control box ──EEBUS (SHIP/SPINE, TLS)──► eebus-bridge ──JSON lines, 127.0.0.1──► gateway [eebus]
 (energy guard)                              (this folder)                            (merges it with IEC 104 and the relay)
```

## What it does

- Approves the limit writes of the paired control box. Under §14a the site has to follow; the gateway never lets the floor fall below Pmin,14a whatever the limit says.
- Applies a limit while it is active and until its duration runs out.
- Keeps the LPC failsafe rules (`state.go`):
  - no heartbeat from the control box within 2 minutes of start, or a heartbeat lost later, means **failsafe**: the failsafe limit applies;
  - the failsafe state ends only with the heartbeat back and either a new limit or the failsafe minimum duration elapsed.
- Sends the gateway one JSON line on every change and at least every 5 s:

  ```json
  {"active":true,"limit_w":7000,"failsafe":false,"failsafe_limit_w":4200}
  ```

  A silent bridge is itself a lost control box for the gateway: after `timeout_s` it applies the last failsafe limit.

## Run it

```sh
cd eebus-bridge
go build
./eebus-bridge -remote-ski <SKI of the control box> -gateway 127.0.0.1:4712
```

The first run creates `eebus.crt` and `eebus.key` and prints the local SKI; that SKI is what the control box is paired with. Other flags: `-port` (SHIP port, 4713), `-nominal-max-w` (the most the controllable devices can draw), `-failsafe-w` and `-failsafe-min` (the values until the control box sets its own).

In the gateway's `gateway.toml`:

```toml
[eebus]
bind = "127.0.0.1:4712"
timeout_s = 15
```

## Tests

```sh
go test ./...                                   # the LPC state machine
go test -tags e2e -run TestEnergyGuard -v ./... # a real EEBUS session
```

The end-to-end test starts an energy guard built on eebus-go's own LPC client, as in its control box example, next to the bridge on the same machine. The two find each other over mDNS, pair over SHIP, and the guard writes a 7,000 W limit. The test passes when the limit arrives at a stand-in gateway socket. It needs mDNS on the machine; CI runs it on Ubuntu.

## Limits

- **Tested against eebus-go, not against a certified control box.** The guard in the test is the same stack on the other side. A real FNN control box has its own EEBUS implementation; pairing and the exact use case versions would have to be checked with one.
- **LPC only.** The other use cases of a German energy manager (LPP for feed-in, MGCP for the grid connection point readings) are not implemented.
- **Pairing by SKI.** The control box's SKI is passed on the command line; there is no pairing user interface.
