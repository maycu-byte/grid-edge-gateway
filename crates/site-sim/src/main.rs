//! Site simulator: every field device of the demo depot as its own Modbus
//! TCP server, driven by the physics in the `devices` crate.
//!
//!   inverter 1  :5020   SunSpec 1/103/120/123
//!   inverter 2  :5021
//!   meter       :5022   SunSpec 1/203
//!   chargers    :5023-5026
//!   heat pump   :5027
//!
//! A small HTTP endpoint (default :8090) exposes the true physical state
//! and lets you take devices offline to test the gateway's fallbacks:
//!   GET  /state
//!   POST /device/{inverter0|meter|charger2|heatpump0}/{offline|online}

use std::future;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use devices::sim::{DeviceId, Exception, SiteSim};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_modbus::server::tcp::{Server, accept_tcp_connection};
use tokio_modbus::{ExceptionCode, Request, Response, SlaveRequest};

type Shared = Arc<Mutex<SiteSim>>;

struct Args {
    base_port: u16,
    http_port: u16,
    start_s: f64,
    speed: f64,
    seed: u64,
}

fn parse_args() -> Args {
    let mut a = Args { base_port: 5020, http_port: 8090, start_s: 6.0 * 3600.0, speed: 1.0, seed: 7 };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    for pair in argv.chunks(2) {
        let [k, v] = pair else { usage() };
        match k.as_str() {
            "--base-port" => a.base_port = v.parse().unwrap_or_else(|_| usage()),
            "--http-port" => a.http_port = v.parse().unwrap_or_else(|_| usage()),
            "--speed" => a.speed = v.parse().unwrap_or_else(|_| usage()),
            "--seed" => a.seed = v.parse().unwrap_or_else(|_| usage()),
            "--start" => {
                let (h, m) = v.split_once(':').unwrap_or_else(|| usage());
                a.start_s = h.parse::<f64>().unwrap_or_else(|_| usage()) * 3600.0
                    + m.parse::<f64>().unwrap_or_else(|_| usage()) * 60.0;
            }
            _ => usage(),
        }
    }
    a
}

fn usage() -> ! {
    eprintln!("usage: site-sim [--start HH:MM] [--speed N] [--seed N] [--base-port 5020] [--http-port 8090]");
    std::process::exit(2)
}

/// One Modbus server per device. An offline device stays silent, so the
/// gateway sees a timeout — like an unplugged cable, not a polite error.
struct DeviceService {
    sim: Shared,
    dev: DeviceId,
}

impl tokio_modbus::server::Service for DeviceService {
    type Request = SlaveRequest<'static>;
    type Response = Option<Response>;
    type Exception = ExceptionCode;
    type Future = future::Ready<Result<Self::Response, Self::Exception>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        let mut sim = self.sim.lock().unwrap();
        let result = match req.request {
            Request::ReadHoldingRegisters(addr, n) => sim.read(self.dev, addr, n).map(Response::ReadHoldingRegisters),
            Request::WriteSingleRegister(addr, v) => {
                sim.write(self.dev, addr, &[v]).map(|_| Response::WriteSingleRegister(addr, v))
            }
            Request::WriteMultipleRegisters(addr, values) => {
                sim.write(self.dev, addr, &values).map(|_| Response::WriteMultipleRegisters(addr, values.len() as u16))
            }
            _ => return future::ready(Err(ExceptionCode::IllegalFunction)),
        };
        future::ready(match result {
            Ok(r) => Ok(Some(r)),
            Err(Exception::DeviceOffline) => Ok(None),
            Err(Exception::IllegalDataAddress) => Err(ExceptionCode::IllegalDataAddress),
            Err(Exception::IllegalDataValue) => Err(ExceptionCode::IllegalDataValue),
        })
    }
}

fn devices(sim: &SiteSim) -> Vec<DeviceId> {
    let mut d: Vec<DeviceId> = (0..sim.inverters.len()).map(DeviceId::Inverter).collect();
    d.push(DeviceId::Meter);
    d.extend((0..sim.chargers.len()).map(DeviceId::Charger));
    d.extend((0..sim.heat_pumps.len()).map(DeviceId::HeatPump));
    d
}

fn device_name(dev: DeviceId) -> String {
    match dev {
        DeviceId::Inverter(i) => format!("inverter{i}"),
        DeviceId::Meter => "meter".into(),
        DeviceId::Charger(i) => format!("charger{i}"),
        DeviceId::HeatPump(i) => format!("heatpump{i}"),
    }
}

async fn serve_device(sim: Shared, dev: DeviceId, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    let server = Server::new(listener);
    let on_connected = |stream, addr| {
        let sim = sim.clone();
        async move { accept_tcp_connection(stream, addr, move |_| Ok(Some(DeviceService { sim: sim.clone(), dev }))) }
    };
    server.serve(&on_connected, |e| eprintln!("modbus error: {e}")).await
}

fn state_json(sim: &SiteSim) -> String {
    let chargers: Vec<String> = sim
        .chargers
        .iter()
        .map(|c| {
            format!(
                r#"{{"online":{},"car":{},"limit_a":{:.1},"current_a":{:.1},"kw":{:.2},"failsafe":{}}}"#,
                c.online,
                c.car.is_some(),
                c.limit_a,
                c.current_a,
                c.power_kw(),
                c.in_failsafe(sim.t_s)
            )
        })
        .collect();
    let hp = &sim.heat_pumps[0];
    format!(
        r#"{{"t_s":{:.0},"grid_kw":{:.2},"pv_kw":{:.2},"base_kw":{:.2},"solar_fraction":{:.3},"outdoor_c":{:.1},"chargers":[{}],"heat_pump":{{"kw":{:.2},"demand_kw":{:.2},"limit_kw":{:.1}}},"inverter_limits":[{}],"meter_online":{}}}"#,
        sim.t_s,
        sim.grid_kw(),
        sim.pv_kw(),
        sim.base_kw,
        sim.solar_fraction(),
        sim.outdoor_c(),
        chargers.join(","),
        hp.power_kw,
        hp.demand_kw,
        hp.limit_kw,
        sim.inverters
            .iter()
            .map(|i| format!("{:.1}", if i.limit_enabled { i.limit_pct } else { 100.0 }))
            .collect::<Vec<_>>()
            .join(","),
        sim.meter_online,
    )
}

/// Deliberately tiny HTTP/1.1 handler: two routes, no framework needed.
async fn serve_http(sim: Shared, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        let sim = sim.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            let n = stream.read(&mut buf).await.unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);
            let line = req.lines().next().unwrap_or("");
            let parts: Vec<&str> = line.split_whitespace().collect();
            let (status, body) = match parts.as_slice() {
                ["GET", "/state", ..] => ("200 OK", state_json(&sim.lock().unwrap())),
                ["POST", path, ..] if path.starts_with("/device/") => {
                    let seg: Vec<&str> = path.trim_start_matches("/device/").split('/').collect();
                    let mut s = sim.lock().unwrap();
                    let target = devices(&s).into_iter().find(|d| seg.first() == Some(&device_name(*d).as_str()));
                    match (target, seg.get(1)) {
                        (Some(d), Some(&"offline")) => {
                            s.set_online(d, false);
                            ("200 OK", r#"{"ok":true}"#.to_string())
                        }
                        (Some(d), Some(&"online")) => {
                            s.set_online(d, true);
                            ("200 OK", r#"{"ok":true}"#.to_string())
                        }
                        _ => ("404 Not Found", r#"{"error":"unknown device or action"}"#.to_string()),
                    }
                }
                _ => ("404 Not Found", r#"{"error":"not found"}"#.to_string()),
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        });
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let sim: Shared = Arc::new(Mutex::new(SiteSim::depot(args.start_s, args.seed)));

    let devs = devices(&sim.lock().unwrap());
    for (k, dev) in devs.into_iter().enumerate() {
        let port = args.base_port + k as u16;
        println!("{:<10} modbus tcp 127.0.0.1:{port}", device_name(dev));
        let sim = sim.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_device(sim, dev, port).await {
                eprintln!("{}: {e}", device_name(dev));
                std::process::exit(1);
            }
        });
    }
    println!("state      http://127.0.0.1:{}/state", args.http_port);
    tokio::spawn(serve_http(sim.clone(), args.http_port));

    // Physics at 10 Hz of wall time; `speed` stretches simulated time.
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tick.tick().await;
        sim.lock().unwrap().step(0.1 * args.speed);
    }
}
