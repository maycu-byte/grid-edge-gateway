"""Interoperability tests: the gateway against c104, a Python binding of
lib60870 (Fraunhofer / MZ Automation) acting as the DSO control centre.

Every test starts its own site simulator and gateway on free ports, so the
suite runs anywhere the two binaries are built (`cargo build`).
"""

import json
import os
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import c104
import pytest

ROOT = Path(__file__).resolve().parents[1]
EXE = ".exe" if sys.platform == "win32" else ""
TARGET = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")) / "debug"
CERTS = ROOT / "certs" / "demo"

GRID, PV, STEUVE, STEUVE_GRID, PMIN, FEED_IN_FB, PV_LIMIT = 1001, 1002, 1003, 1004, 1005, 1006, 1007
FEED_IN_IN_FORCE, BATTERY_KW, BATTERY_SOC, DIM_MINUTES, CURTAILED_KWH, BUDGET_USED = 1008, 1009, 1010, 1011, 1012, 1013
DIMMED, RELEASING, METER_FALLBACK, DEVICE_FAULT = 2001, 2002, 2003, 2004
EMERGENCY_ACTIVE, DAY_LIMIT, BUDGET_EXHAUSTED = 2005, 2006, 2007
CMD_DIM, CMD_FEED_IN, CMD_EMERGENCY = 5001, 5002, 5003
PMIN_DEPOT = 18.2  # 4 chargers + battery + 14 kW heat pump: 0.4 * 14 + 5 * 0.6 * 4.2


def free_port_block(n):
    """A base port with n free consecutive ports after it."""
    for _ in range(50):
        with socket.socket() as s:
            s.bind(("127.0.0.1", 0))
            base = s.getsockname()[1]
        if base + n >= 65535:
            continue
        ok = True
        for p in range(base, base + n):
            with socket.socket() as t:
                try:
                    t.bind(("127.0.0.1", p))
                except OSError:
                    ok = False
                    break
        if ok:
            return base
    raise RuntimeError("no free port block")


def wait_port(port, timeout=15):
    end = time.time() + timeout
    while time.time() < end:
        with socket.socket() as s:
            if s.connect_ex(("127.0.0.1", port)) == 0:
                return
        time.sleep(0.1)
    raise TimeoutError(f"port {port} did not open")


def wait_until(pred, timeout=10, step=0.2):
    end = time.time() + timeout
    while time.time() < end:
        if pred():
            return True
        time.sleep(step)
    return False


class Site:
    """site-sim + gateway, with a generated config."""

    def __init__(
        self, tmp: Path, tls: bool, start="17:40", jurisdiction="DE", site_extra="", policy="", iec_extra="", extra=""
    ):
        self.tmp = tmp
        self.tls = tls
        self.base = free_port_block(12)
        self.http = self.base + 9
        self.iec = self.base + 10
        self.api = self.base + 11
        self.jurisdiction = jurisdiction
        self.site_extra = site_extra
        self.policy = policy
        self.iec_extra = iec_extra
        self.extra = extra
        self.start = start
        self.procs = {}

    def config(self):
        b = self.base
        chargers = "".join(
            f'[[charger]]\naddress = "127.0.0.1:{b + 3 + i}"\nmax_current_a = 32.0\n'
            f"failsafe_current_a = 6.0\nfailsafe_timeout_s = 30\n"
            for i in range(4)
        )
        tls = ""
        if self.tls:
            tls = (
                "[iec104.tls]\n"
                f'cert = "{(CERTS / "station.pem").as_posix()}"\n'
                f'key = "{(CERTS / "station.key.pem").as_posix()}"\n'
                f'client_ca = "{(CERTS / "ca.pem").as_posix()}"\n'
            )
        return f"""
[site]
jurisdiction = "{self.jurisdiction}"
pv_installed_kw = 120.0
connection_kw = 250.0
pv_ramp_pct_per_s = 20.0
{self.site_extra}
feed_in_reference = "grid_connection_point"
release_ramp_s = 20.0
margin_kw = 0.3
min_dwell_s = 300.0
control_period_ms = 500
stale_after_ms = 1500

{self.policy}

[[inverter]]
address = "127.0.0.1:{b}"
[[inverter]]
address = "127.0.0.1:{b + 1}"
[meter]
address = "127.0.0.1:{b + 2}"
{chargers}
[[heat_pump]]
address = "127.0.0.1:{b + 7}"
rated_kw = 14.0
min_kw = 3.0

[[battery]]
address = "127.0.0.1:{b + 8}"
capacity_kwh = 100.0
max_charge_kw = 50.0
max_discharge_kw = 50.0
min_soc_pct = 10.0
max_soc_pct = 95.0
watchdog_s = 30

[iec104]
bind = "127.0.0.1:{self.iec}"
common_address = 1
deadband_kw = 0.5
{self.iec_extra}
{tls}
[api]
bind = "127.0.0.1:{self.api}"
{self.extra}
"""

    def start_sim(self):
        self.procs["sim"] = subprocess.Popen(
            [TARGET / f"site-sim{EXE}", "--start", self.start, "--base-port", str(self.base), "--http-port", str(self.http)],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        wait_port(self.http)

    def start_gateway(self):
        cfg = self.tmp / "gateway.toml"
        cfg.write_text(self.config())
        self.procs["gw"] = subprocess.Popen(
            [TARGET / f"gateway{EXE}", str(cfg)],
            stdout=open(self.tmp / "gateway.log", "ab"),
            stderr=subprocess.STDOUT,
        )
        wait_port(self.api)
        wait_port(self.iec)
        # First control cycles with all devices online.
        assert wait_until(lambda: self.snapshot()["fallbacks"] == [] and self.snapshot()["grid_kw"] is not None)

    def stop_gateway(self):
        p = self.procs.pop("gw")
        p.kill()
        p.wait()

    def snapshot(self):
        with urllib.request.urlopen(f"http://127.0.0.1:{self.api}/api/snapshot", timeout=2) as r:
            return json.load(r)

    def plan(self):
        with urllib.request.urlopen(f"http://127.0.0.1:{self.api}/api/plan", timeout=2) as r:
            return json.load(r)

    def sim_device(self, name, action):
        req = urllib.request.Request(f"http://127.0.0.1:{self.http}/device/{name}/{action}", method="POST")
        urllib.request.urlopen(req, timeout=2).read()

    def close(self):
        for p in self.procs.values():
            p.kill()
            p.wait()


class Dso:
    """The control centre side, built on c104."""

    def __init__(self, port, tls=None):
        self.client = c104.Client(tick_rate_ms=50, command_timeout_ms=3000, transport_security=tls)
        self.conn = self.client.add_connection(ip="127.0.0.1", port=port, init=c104.Init.ALL)
        self.station = self.conn.add_station(common_address=1)

        def on_new_point(client: c104.Client, station: c104.Station, io_address: int, point_type: c104.Type) -> None:
            station.add_point(io_address=io_address, type=point_type)

        self.client.on_new_point(callable=on_new_point)
        self.dim = self.station.add_point(io_address=CMD_DIM, type=c104.Type.C_SC_NA_1)
        self.feed_in = self.station.add_point(io_address=CMD_FEED_IN, type=c104.Type.C_SE_NC_1)
        self.emergency = self.station.add_point(io_address=CMD_EMERGENCY, type=c104.Type.C_SC_NA_1)
        self.client.start()

    def connected(self, timeout=5):
        return wait_until(lambda: self.conn.is_connected, timeout)

    def point(self, ioa):
        return self.station.get_point(io_address=ioa)

    def value(self, ioa):
        p = self.point(ioa)
        return None if p is None else p.value

    def close(self):
        self.client.stop()


@pytest.fixture
def site(tmp_path):
    s = Site(tmp_path, tls=False)
    s.start_sim()
    s.start_gateway()
    yield s
    s.close()


@pytest.fixture
def dso(site):
    d = Dso(site.iec)
    assert d.connected()
    # init=ALL: the client runs a general interrogation right after STARTDT.
    assert wait_until(lambda: d.point(DEVICE_FAULT) is not None, 5)
    yield d
    d.close()


def test_general_interrogation_returns_the_point_list(dso):
    for ioa in (GRID, PV, STEUVE, STEUVE_GRID, PMIN, FEED_IN_FB, PV_LIMIT):
        p = dso.point(ioa)
        assert p is not None and p.type == c104.Type.M_ME_TF_1, ioa
        assert p.quality.is_good(), ioa
    for ioa in (DIMMED, RELEASING, METER_FALLBACK, DEVICE_FAULT):
        assert dso.point(ioa).type == c104.Type.M_SP_TB_1
    assert dso.value(PMIN) == pytest.approx(PMIN_DEPOT, abs=0.01)
    for ioa in (FEED_IN_IN_FORCE, BATTERY_KW, BATTERY_SOC, DIM_MINUTES, CURTAILED_KWH):
        assert dso.point(ioa).quality.is_good(), ioa
    assert dso.point(BUDGET_USED).quality.is_good() is False, "no curtailment budget in DE"
    for ioa in (EMERGENCY_ACTIVE, DAY_LIMIT, BUDGET_EXHAUSTED):
        assert dso.value(ioa) is False
    assert dso.value(DIMMED) is False


def test_dimming_keeps_grid_draw_of_controllable_devices_under_pmin(site, dso):
    # 17:40 on the simulated day: four cars and the heat pump draw far more than Pmin.
    assert dso.value(STEUVE_GRID) > 40
    dso.dim.value = True
    assert dso.dim.transmit(cause=c104.Cot.ACTIVATION)
    assert wait_until(lambda: dso.value(DIMMED) is True, 5)
    # Chargers and heat pump follow within a few control cycles.
    assert wait_until(lambda: dso.value(STEUVE_GRID) <= PMIN_DEPOT, 8), dso.value(STEUVE_GRID)
    snap = site.snapshot()
    assert snap["mode"] == "dimmed"
    active = [c for c in snap["chargers"] if c["setpoint_a"] > 0]
    assert all(c["setpoint_a"] >= 6 for c in active)

    dso.dim.value = False
    assert dso.dim.transmit(cause=c104.Cot.ACTIVATION)
    assert wait_until(lambda: dso.value(RELEASING) is True, 5)
    # Gradual release: no step back to full power.
    time.sleep(3)
    assert site.snapshot()["steuve_budget_kw"] < 60
    assert wait_until(lambda: dso.value(RELEASING) is False, 30)


def test_feed_in_limit_is_confirmed_and_out_of_range_is_rejected(dso):
    dso.feed_in.value = 30.0
    assert dso.feed_in.transmit(cause=c104.Cot.ACTIVATION)
    assert wait_until(lambda: dso.value(FEED_IN_FB) == pytest.approx(30.0), 5)
    dso.feed_in.value = 150.0
    assert not dso.feed_in.transmit(cause=c104.Cot.ACTIVATION), "negative confirmation expected"
    assert dso.value(FEED_IN_FB) == pytest.approx(30.0)


def test_command_to_unknown_address_is_rejected(dso):
    bogus = dso.station.add_point(io_address=5999, type=c104.Type.C_SC_NA_1)
    bogus.value = True
    assert not bogus.transmit(cause=c104.Cot.ACTIVATION)


def test_meter_loss_is_reported_and_dimming_falls_back_to_pmin(site, dso):
    dso.dim.value = True
    assert dso.dim.transmit(cause=c104.Cot.ACTIVATION)
    site.sim_device("meter", "offline")
    assert wait_until(lambda: dso.value(METER_FALLBACK) is True, 6)
    assert dso.point(GRID).quality.is_good() is False
    snap = site.snapshot()
    assert snap["steuve_budget_kw"] == pytest.approx(PMIN_DEPOT - 0.3, abs=0.01)
    site.sim_device("meter", "online")
    assert wait_until(lambda: dso.value(METER_FALLBACK) is False, 6)


def test_dso_commands_survive_a_gateway_restart(site, dso):
    dso.dim.value = True
    assert dso.dim.transmit(cause=c104.Cot.ACTIVATION)
    assert wait_until(lambda: site.snapshot()["dso"]["dim"], 5)
    dso.close()
    site.stop_gateway()
    site.start_gateway()
    snap = site.snapshot()
    assert snap["dso"]["dim"] is True
    assert snap["mode"] == "dimmed"


# --- robustness and country rules -------------------------------------------------


def test_emergency_command_is_confirmed_and_reported(dso):
    dso.emergency.value = True
    assert dso.emergency.transmit(cause=c104.Cot.ACTIVATION)
    assert wait_until(lambda: dso.value(EMERGENCY_ACTIVE) is True, 5)
    dso.emergency.value = False
    assert dso.emergency.transmit(cause=c104.Cot.ACTIVATION)
    assert wait_until(lambda: dso.value(EMERGENCY_ACTIVE) is False, 5)


def test_battery_discharges_to_support_dimmed_loads(site, dso):
    dso.dim.value = True
    assert dso.dim.transmit(cause=c104.Cot.ACTIVATION)
    # 17:40, no sun: the battery covers part of the evening import.
    assert wait_until(lambda: (dso.value(BATTERY_KW) or 0) < -10, 10), dso.value(BATTERY_KW)
    snap = site.snapshot()
    loads = sum(c["kw"] or 0 for c in snap["chargers"]) + sum(h["kw"] or 0 for h in snap["heat_pumps"])
    assert loads > PMIN_DEPOT, "loads get more than Pmin thanks to the battery"


def test_battery_offline_is_a_fallback_and_it_idles_on_its_own(site, dso):
    site.sim_device("battery0", "offline")
    assert wait_until(lambda: dso.value(DEVICE_FAULT) is True, 6)
    assert "battery0 offline" in site.snapshot()["fallbacks"]
    site.sim_device("battery0", "online")
    assert wait_until(lambda: dso.value(DEVICE_FAULT) is False, 8)


def test_austria_applies_the_static_70_percent_cap(tmp_path):
    s = Site(tmp_path, tls=False, start="12:30", jurisdiction="AT", policy="[policy]\nstatic_feed_in_cap_pct = 70.0\n")
    try:
        s.start_sim()
        s.start_gateway()
        d = Dso(s.iec)
        try:
            assert d.connected()
            assert wait_until(lambda: d.value(FEED_IN_IN_FORCE) == pytest.approx(70.0), 5)
            assert d.value(FEED_IN_FB) == pytest.approx(100.0), "no DSO command was sent"
        finally:
            d.close()
    finally:
        s.close()


def test_switzerland_refuses_curtailment_beyond_the_3_percent_budget(tmp_path):
    # A tiny yield and budget (0.5% of 1 kWh) run out in seconds; at noon the
    # battery and the depot absorb most of the PV, so little is curtailed.
    s = Site(
        tmp_path,
        tls=False,
        start="12:30",
        jurisdiction="CH",
        site_extra="expected_annual_yield_kwh = 1.0",
        policy="[policy]\ncurtailment_budget_pct = 0.5\n",
    )
    try:
        s.start_sim()
        s.start_gateway()
        d = Dso(s.iec)
        try:
            assert d.connected()
            assert wait_until(lambda: d.point(BUDGET_USED) is not None, 5)
            # Without the battery soaking up the surplus, PV is really curtailed.
            s.sim_device("battery0", "offline")
            d.feed_in.value = 0.0
            assert d.feed_in.transmit(cause=c104.Cot.ACTIVATION)
            assert wait_until(lambda: d.value(BUDGET_EXHAUSTED) is True, 60), s.snapshot()["totals"]
            assert wait_until(lambda: d.value(FEED_IN_IN_FORCE) == pytest.approx(100.0), 5)
            # An emergency still curtails.
            d.emergency.value = True
            assert d.emergency.transmit(cause=c104.Cot.ACTIVATION)
            assert wait_until(lambda: d.value(FEED_IN_IN_FORCE) == pytest.approx(0.0), 5)
        finally:
            d.close()
    finally:
        s.close()


def test_link_loss_policy_release_lifts_commands(tmp_path):
    s = Site(tmp_path, tls=False, iec_extra='on_link_loss = "release"\nlink_loss_release_s = 2')
    try:
        s.start_sim()
        s.start_gateway()
        d = Dso(s.iec)
        assert d.connected()
        d.dim.value = True
        assert d.dim.transmit(cause=c104.Cot.ACTIVATION)
        assert wait_until(lambda: s.snapshot()["dso"]["dim"], 5)
        d.close()
        assert wait_until(lambda: s.snapshot()["dso"]["dim"] is False, 10)
    finally:
        s.close()


def test_state_file_keeps_the_totals(site, dso):
    dso.dim.value = True
    assert dso.dim.transmit(cause=c104.Cot.ACTIVATION)
    time.sleep(2)
    dso.dim.value = False
    assert dso.dim.transmit(cause=c104.Cot.ACTIVATION)
    state = json.loads((site.tmp / "dso-state.json").read_text())
    assert state["totals"]["dimmed_s_today"] > 0


# --- TLS -------------------------------------------------------------------

def test_planner_defers_charging_to_cheap_hours_and_the_floor_still_holds(tmp_path):
    # Day-ahead prices from a file: 500 €/MWh for the next three hours, 50 after.
    now = int(time.time()) // 900 * 900
    prices = tmp_path / "prices.csv"
    prices.write_text(
        "start,eur_mwh\n" + "".join(f"{now - 3600 + k * 900},{500.0 if k < 16 else 50.0}\n" for k in range(160))
    )
    planner = (
        "[planner]\n[planner.prices]\nsources = [\"file\"]\n"
        f'file = "{prices.as_posix()}"\nimport_adder_eur_kwh = 0.12\n'
    )
    s = Site(tmp_path, tls=False, extra=planner)
    try:
        s.start_sim()
        s.start_gateway()
        assert wait_until(lambda: s.snapshot()["planner"]["status"] == "following the plan", 20), s.snapshot()[
            "planner"
        ]
        plan = s.plan()
        assert len(plan["grid_kw"]) == 96
        assert plan["price_eur_mwh"][0] == 500.0
        # 17:40: three vans stay overnight and one leaves at 20:30. The plan
        # charges the overnight vans after the expensive hours; the rules
        # alone would charge all four at once.
        assert wait_until(
            lambda: sum(1 for c in s.snapshot()["chargers"] if c["status"] == "waiting" and c["setpoint_a"] == 0) >= 2,
            10,
        ), s.snapshot()["chargers"]
        assert (tmp_path / "planner-state.json").exists()

        d = Dso(s.iec)
        try:
            assert d.connected()
            d.dim.value = True
            assert d.dim.transmit(cause=c104.Cot.ACTIVATION)
            assert wait_until(lambda: d.value(DIMMED) is True, 5)
            assert wait_until(lambda: d.value(STEUVE_GRID) <= PMIN_DEPOT, 8), d.value(STEUVE_GRID)
            # the planner re-plans for the dimming and the site keeps following a plan
            assert wait_until(lambda: s.plan()["dim"][0] is True, 15)
            assert d.value(STEUVE_GRID) <= PMIN_DEPOT
        finally:
            d.close()
    finally:
        s.close()


@pytest.fixture
def tls_site(tmp_path):
    if not (CERTS / "ca.pem").exists():
        subprocess.run(["sh", str(ROOT / "certs" / "gen-demo.sh")], check=True)
    s = Site(tmp_path, tls=True)
    s.start_sim()
    s.start_gateway()
    yield s
    s.close()


# c104 2.2.1 cannot be used as the TLS client here: its bundled mbedtls 3.6
# refuses to verify a server without a hostname
# (MBEDTLS_ERR_SSL_CERTIFICATE_VERIFICATION_WITHOUT_HOSTNAME, -0x5D80) and the
# released binding has no way to set one yet. The TLS tests therefore speak
# raw IEC 104 frames through Python's ssl module (OpenSSL).

STARTDT_ACT = bytes([0x68, 0x04, 0x07, 0x00, 0x00, 0x00])
STARTDT_CON = bytes([0x68, 0x04, 0x0B, 0x00, 0x00, 0x00])
GI = bytes([0x68, 0x0E, 0x00, 0x00, 0x00, 0x00, 0x64, 0x01, 0x06, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x14])


def tls_client(port, cert=None, key=None):
    import ssl

    ctx = ssl.create_default_context(ssl.Purpose.SERVER_AUTH, cafile=str(CERTS / "ca.pem"))
    ctx.minimum_version = ssl.TLSVersion.TLSv1_2
    if cert:
        ctx.load_cert_chain(str(CERTS / cert), str(CERTS / key))
    raw = socket.create_connection(("127.0.0.1", port), timeout=5)
    return ctx.wrap_socket(raw, server_hostname="localhost")


def read_apdus(sock, until, timeout=5):
    """Reads APDUs until `until(list_of_frames)` is true."""
    frames, buf, end = [], b"", time.time() + timeout
    sock.settimeout(0.5)
    while time.time() < end and not until(frames):
        try:
            chunk = sock.recv(4096)
        except (TimeoutError, socket.timeout):
            continue
        if not chunk:
            break
        buf += chunk
        while len(buf) >= 2 and len(buf) >= 2 + buf[1]:
            frames.append(buf[: 2 + buf[1]])
            buf = buf[2 + buf[1]:]
    return frames


def test_tls_with_dso_client_certificate_works(tls_site):
    with tls_client(tls_site.iec, "dso.pem", "dso.key.pem") as s:
        assert s.version() in ("TLSv1.2", "TLSv1.3")
        s.sendall(STARTDT_ACT)
        assert STARTDT_CON in read_apdus(s, lambda f: STARTDT_CON in f)
        s.sendall(GI)
        # I-frames carrying the interrogation answer: actcon, M_ME_TF_1 (36), M_SP_TB_1 (30), actterm
        frames = read_apdus(s, lambda f: any(x[6] == 100 and x[8] & 0x3F == 10 for x in f if len(x) > 8))
        types = [f[6] for f in frames if len(f) > 6 and f[2] & 1 == 0]
        assert 36 in types and 30 in types, types


@pytest.mark.parametrize("cert,key", [("rogue.pem", "rogue.key.pem"), (None, None)])
def test_tls_rejects_clients_without_a_dso_certificate(tls_site, cert, key):
    import ssl

    with pytest.raises((ssl.SSLError, ConnectionError, OSError)):
        with tls_client(tls_site.iec, cert, key) as s:
            # TLS 1.3 reports a rejected client certificate on the first read.
            s.sendall(STARTDT_ACT)
            assert read_apdus(s, lambda f: STARTDT_CON in f, 3) == []
            raise ConnectionError("no data")
    assert tls_site.snapshot()["dso"]["connections"] == 0


def test_plain_tcp_client_cannot_talk_to_a_tls_station(tls_site):
    with socket.create_connection(("127.0.0.1", tls_site.iec), timeout=5) as s:
        s.sendall(STARTDT_ACT)
        assert STARTDT_CON not in read_apdus(s, lambda f: STARTDT_CON in f, 3)
