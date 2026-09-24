//! Field bus: one task per Modbus device. Each task connects (and keeps
//! reconnecting), discovers the SunSpec models where there are any, polls
//! the device every control period and writes the latest setpoints.
//!
//! Tasks never block each other: a device that stops answering only makes
//! its own reading go stale, which the controller treats as "offline".

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use devices::maps::{battery, evse, heat_pump, regs_to_u32};
use devices::sunspec::{self, ModelLocation, available, controls, inverter, meter, nameplate};
use tokio::sync::watch;
use tokio::time::timeout;
use tokio_modbus::Slave;
use tokio_modbus::client::{Context, Reader, Writer};
use tracing::{info, warn};

use crate::config::{Battery, Charger, Config, Device, HeatPump};

const IO_TIMEOUT: Duration = Duration::from_millis(800);
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// Inverter limits are re-written this often even without a change, so a
/// power-cycled inverter gets its limit back quickly.
const INVERTER_REFRESH: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default)]
pub struct InverterReading {
    pub kw: f64,
    pub rated_kw: f64,
    /// From the vendor model 64900, when the inverter has it.
    pub available_kw: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct ChargerReading {
    pub status: u16,
    pub current_a: f64,
    pub kw: f64,
    pub session_kwh: f64,
}

#[derive(Debug, Clone, Default)]
pub struct HeatPumpReading {
    pub kw: f64,
    pub demand_kw: f64,
}

#[derive(Debug, Clone, Default)]
pub struct BatteryReading {
    pub status: u16,
    pub kw: f64,
    pub soc_pct: f64,
}

/// Latest reading of one device and when it was taken.
#[derive(Debug, Clone)]
pub struct Slot<T> {
    pub value: Option<T>,
    pub at: Option<Instant>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Slot { value: None, at: None }
    }
}

impl<T: Clone> Slot<T> {
    pub fn fresh(&self, max_age: Duration) -> Option<T> {
        match (self.at, &self.value) {
            (Some(at), Some(v)) if at.elapsed() <= max_age => Some(v.clone()),
            _ => None,
        }
    }

    fn set(&mut self, v: T) {
        self.value = Some(v);
        self.at = Some(Instant::now());
    }
}

#[derive(Debug, Default)]
pub struct FieldState {
    pub inverters: Vec<Slot<InverterReading>>,
    pub meter_kw: Slot<f64>,
    pub chargers: Vec<Slot<ChargerReading>>,
    pub heat_pumps: Vec<Slot<HeatPumpReading>>,
    pub batteries: Vec<Slot<BatteryReading>>,
}

pub type Shared = Arc<Mutex<FieldState>>;

type IoResult<T> = Result<T, String>;

async fn read(ctx: &mut Context, addr: u16, n: u16) -> IoResult<Vec<u16>> {
    match timeout(IO_TIMEOUT, ctx.read_holding_registers(addr, n)).await {
        Err(_) => Err(format!("read {addr}: timeout")),
        Ok(Err(e)) => Err(format!("read {addr}: {e}")),
        Ok(Ok(Err(x))) => Err(format!("read {addr}: exception {x:?}")),
        Ok(Ok(Ok(v))) => Ok(v),
    }
}

async fn write(ctx: &mut Context, addr: u16, v: u16) -> IoResult<()> {
    match timeout(IO_TIMEOUT, ctx.write_single_register(addr, v)).await {
        Err(_) => Err(format!("write {addr}: timeout")),
        Ok(Err(e)) => Err(format!("write {addr}: {e}")),
        Ok(Ok(Err(x))) => Err(format!("write {addr}: exception {x:?}")),
        Ok(Ok(Ok(()))) => Ok(()),
    }
}

async fn connect(addr: SocketAddr, unit: u8) -> IoResult<Context> {
    match timeout(IO_TIMEOUT * 2, tokio_modbus::client::tcp::connect_slave(addr, Slave(unit))).await {
        Err(_) => Err("connect: timeout".into()),
        Ok(Err(e)) => Err(format!("connect: {e}")),
        Ok(Ok(ctx)) => Ok(ctx),
    }
}

/// Walks the SunSpec model chain from register 40000.
pub async fn discover(ctx: &mut Context) -> IoResult<Vec<ModelLocation>> {
    if read(ctx, sunspec::BASE, 2).await? != sunspec::MARKER {
        return Err("no SunSpec marker at 40000".into());
    }
    let mut addr = sunspec::BASE + 2;
    let mut models = Vec::new();
    loop {
        let h = read(ctx, addr, 2).await?;
        if h[0] == sunspec::END_ID {
            return Ok(models);
        }
        models.push(ModelLocation { id: h[0], body: addr + 2, len: h[1] });
        addr += 2 + h[1];
        if models.len() > 32 {
            return Err("SunSpec model chain does not end".into());
        }
    }
}

fn find(models: &[ModelLocation], id: u16) -> IoResult<ModelLocation> {
    models.iter().find(|m| m.id == id).copied().ok_or(format!("SunSpec model {id} missing"))
}

/// Runs `session` forever, reconnecting after any error. `on_lost` clears
/// the device's reading so the controller sees it offline at once.
async fn supervise<F, Fut>(name: String, dev: Device, on_lost: impl Fn(), mut session: F)
where
    F: FnMut(Context) -> Fut,
    Fut: std::future::Future<Output = IoResult<()>>,
{
    let mut was_up = true;
    loop {
        let result = match connect(dev.address, dev.unit).await {
            Ok(ctx) => {
                info!("{name}: connected to {}", dev.address);
                was_up = true;
                session(ctx).await
            }
            Err(e) => Err(e),
        };
        on_lost();
        if let Err(e) = result {
            if was_up {
                warn!("{name}: {e}");
            }
            was_up = false;
        }
        tokio::time::sleep(RECONNECT_DELAY).await;
    }
}

pub fn spawn_all(cfg: &Config, state: Shared, setpoints: watch::Receiver<control::Setpoints>, period: Duration) {
    for (i, dev) in cfg.inverters.iter().enumerate() {
        let (state, sp) = (state.clone(), setpoints.clone());
        let lost = {
            let s = state.clone();
            move || s.lock().unwrap().inverters[i] = Slot::default()
        };
        tokio::spawn(supervise(format!("inverter{i}"), dev.clone(), lost, move |ctx| {
            inverter_session(ctx, i, state.clone(), sp.clone(), period)
        }));
    }
    {
        let st = state.clone();
        let lost = {
            let s = state.clone();
            move || s.lock().unwrap().meter_kw = Slot::default()
        };
        tokio::spawn(supervise("meter".into(), cfg.meter.clone(), lost, move |ctx| {
            meter_session(ctx, st.clone(), period)
        }));
    }
    for (i, c) in cfg.chargers.iter().enumerate() {
        let (state, sp, c) = (state.clone(), setpoints.clone(), c.clone());
        let dev = Device { address: c.address, unit: c.unit };
        let lost = {
            let s = state.clone();
            move || s.lock().unwrap().chargers[i] = Slot::default()
        };
        tokio::spawn(supervise(format!("charger{i}"), dev, lost, move |ctx| {
            charger_session(ctx, i, c.clone(), state.clone(), sp.clone(), period)
        }));
    }
    for (i, b) in cfg.batteries.iter().enumerate() {
        let (state, sp, b) = (state.clone(), setpoints.clone(), b.clone());
        let dev = Device { address: b.address, unit: b.unit };
        let lost = {
            let s = state.clone();
            move || s.lock().unwrap().batteries[i] = Slot::default()
        };
        tokio::spawn(supervise(format!("battery{i}"), dev, lost, move |ctx| {
            battery_session(ctx, i, b.clone(), state.clone(), sp.clone(), period)
        }));
    }
    for (i, h) in cfg.heat_pumps.iter().enumerate() {
        let (state, sp, h) = (state.clone(), setpoints.clone(), h.clone());
        let dev = Device { address: h.address, unit: h.unit };
        let lost = {
            let s = state.clone();
            move || s.lock().unwrap().heat_pumps[i] = Slot::default()
        };
        tokio::spawn(supervise(format!("heatpump{i}"), dev, lost, move |ctx| {
            heat_pump_session(ctx, i, h.clone(), state.clone(), sp.clone(), period)
        }));
    }
}

async fn inverter_session(
    mut ctx: Context,
    i: usize,
    state: Shared,
    setpoints: watch::Receiver<control::Setpoints>,
    period: Duration,
) -> IoResult<()> {
    let models = discover(&mut ctx).await?;
    let inv = find(&models, inverter::ID)?;
    let ctl = find(&models, controls::ID)?;
    let np = find(&models, nameplate::ID)?;
    let avail = models.iter().find(|m| m.id == available::ID).copied();
    let n = read(&mut ctx, np.body, nameplate::LEN as u16).await?;
    let rated_kw = sunspec::scaled(n[nameplate::W_RTG] as i16, n[nameplate::W_RTG_SF] as i16) / 1000.0;
    let k = read(&mut ctx, ctl.body, controls::LEN as u16).await?;
    let pct_sf = k[controls::W_MAX_LIM_PCT_SF] as i16;
    info!("inverter{i}: SunSpec models {:?}, rating {rated_kw} kW", models.iter().map(|m| m.id).collect::<Vec<_>>());

    // Never let the limit revert on its own: if this gateway dies, the last
    // DSO limit must stay in force (fail-safe towards the grid).
    write(&mut ctx, ctl.body + controls::W_MAX_LIM_PCT_RVRT_TMS as u16, 0).await?;

    let mut written: Option<(f64, Instant)> = None;
    let mut tick = tokio::time::interval(period);
    loop {
        tick.tick().await;
        let m = read(&mut ctx, inv.body, inverter::LEN as u16).await?;
        let kw = sunspec::scaled(m[inverter::W] as i16, m[inverter::W_SF] as i16) / 1000.0;
        let available_kw = match avail {
            Some(a) => {
                let r = read(&mut ctx, a.body, available::LEN as u16).await?;
                Some(sunspec::scaled(r[available::W_AVAIL] as i16, r[available::W_AVAIL_SF] as i16) / 1000.0)
            }
            None => None,
        };

        let target = setpoints.borrow().pv_limit_pct;
        let due = match written {
            None => true,
            Some((pct, at)) => (pct - target).abs() > 0.05 || at.elapsed() > INVERTER_REFRESH,
        };
        if due {
            let raw = sunspec::unscaled(target, pct_sf) as u16;
            write(&mut ctx, ctl.body + controls::W_MAX_LIM_PCT as u16, raw).await?;
            write(&mut ctx, ctl.body + controls::W_MAX_LIM_ENA as u16, 1).await?;
            written = Some((target, Instant::now()));
        }
        state.lock().unwrap().inverters[i].set(InverterReading { kw, rated_kw, available_kw });
    }
}

async fn meter_session(mut ctx: Context, state: Shared, period: Duration) -> IoResult<()> {
    let models = discover(&mut ctx).await?;
    let mt = find(&models, meter::ID)?;
    let mut tick = tokio::time::interval(period);
    loop {
        tick.tick().await;
        let m = read(&mut ctx, mt.body, meter::LEN as u16).await?;
        let kw = sunspec::scaled(m[meter::W] as i16, m[meter::W_SF] as i16) / 1000.0;
        state.lock().unwrap().meter_kw.set(kw);
    }
}

async fn charger_session(
    mut ctx: Context,
    i: usize,
    cfg: Charger,
    state: Shared,
    setpoints: watch::Receiver<control::Setpoints>,
    period: Duration,
) -> IoResult<()> {
    // Arm the charger's own watchdog first: from now on, if the gateway goes
    // quiet for `failsafe_timeout_s`, the charger drops to the failsafe current.
    write(&mut ctx, evse::FAILSAFE_CURRENT, (cfg.failsafe_current_a * 10.0) as u16).await?;
    write(&mut ctx, evse::FAILSAFE_TIMEOUT, cfg.failsafe_timeout_s).await?;
    let mut tick = tokio::time::interval(period);
    loop {
        tick.tick().await;
        let r = read(&mut ctx, 0, evse::LEN).await?;
        let reading = ChargerReading {
            status: r[evse::STATUS as usize],
            current_a: r[evse::CURRENT as usize] as f64 / 10.0,
            kw: regs_to_u32(&r[evse::POWER as usize..]) as f64 / 1000.0,
            session_kwh: regs_to_u32(&r[evse::SESSION_ENERGY as usize..]) as f64 / 1000.0,
        };
        state.lock().unwrap().chargers[i].set(reading);
        // Written every cycle: it is also the heartbeat.
        let a = setpoints.borrow().charger_current_a.get(i).copied().unwrap_or(0.0);
        write(&mut ctx, evse::CURRENT_LIMIT, (a * 10.0).round() as u16).await?;
    }
}

async fn heat_pump_session(
    mut ctx: Context,
    i: usize,
    _cfg: HeatPump,
    state: Shared,
    setpoints: watch::Receiver<control::Setpoints>,
    period: Duration,
) -> IoResult<()> {
    let mut tick = tokio::time::interval(period);
    loop {
        tick.tick().await;
        let r = read(&mut ctx, 0, heat_pump::LEN).await?;
        state.lock().unwrap().heat_pumps[i].set(HeatPumpReading {
            kw: r[heat_pump::POWER as usize] as f64 / 10.0,
            demand_kw: r[heat_pump::DEMAND as usize] as f64 / 10.0,
        });
        let kw = setpoints.borrow().heat_pump_limit_kw.get(i).copied().unwrap_or(0.0);
        write(&mut ctx, heat_pump::POWER_LIMIT, (kw * 10.0).floor() as u16).await?;
    }
}

async fn battery_session(
    mut ctx: Context,
    i: usize,
    cfg: Battery,
    state: Shared,
    setpoints: watch::Receiver<control::Setpoints>,
    period: Duration,
) -> IoResult<()> {
    // Arm the BMS watchdog: if the gateway goes quiet, the battery idles.
    write(&mut ctx, battery::WATCHDOG_S, cfg.watchdog_s).await?;
    let mut tick = tokio::time::interval(period);
    loop {
        tick.tick().await;
        let r = read(&mut ctx, 0, battery::LEN).await?;
        state.lock().unwrap().batteries[i].set(BatteryReading {
            status: r[battery::STATUS as usize],
            kw: r[battery::POWER as usize] as i16 as f64 / 10.0,
            soc_pct: r[battery::SOC as usize] as f64 / 10.0,
        });
        // Written every cycle: it is also the heartbeat.
        let kw = setpoints.borrow().battery_kw.get(i).copied().unwrap_or(0.0);
        let raw = (kw * 10.0).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16;
        write(&mut ctx, battery::SETPOINT, raw as u16).await?;
    }
}
