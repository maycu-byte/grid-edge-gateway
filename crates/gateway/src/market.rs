//! Day-ahead prices for the planner, from the ENTSO-E Transparency Platform
//! (needs a security token), Energy-Charts (Fraunhofer ISE, open data from
//! SMARD), or a CSV file.
//!
//! All three give the single day-ahead coupling price (SDAC) per bidding
//! zone: in 15-minute products since 1 October 2025 in DE-LU and AT, hourly
//! in CH. The parsers keep each price with its own duration.

use std::collections::BTreeMap;
use std::time::Duration;

use planning::unix_from_utc;

const ENTSOE_API: &str = "https://web-api.tp.entsoe.eu/api";
const ENERGY_CHARTS_API: &str = "https://api.energy-charts.info/price";

/// One day-ahead price: `[start_s, start_s + dur_s)`, Unix seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PricePoint {
    pub start_s: i64,
    pub dur_s: i64,
    pub eur_mwh: f64,
}

/// ENTSO-E's code (EIC) for a bidding zone.
pub fn eic(zone: &str) -> Option<&'static str> {
    Some(match zone {
        "DE-LU" => "10Y1001A1001A82H",
        "AT" => "10YAT-APG------L",
        "CH" => "10YCH-SWISSGRIDZ",
        _ => return None,
    })
}

/// "2025-01-19T23:00Z" or "2025-01-19T23:00:00Z" → Unix seconds.
fn parse_utc(s: &str) -> Option<i64> {
    let s = s.trim().strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<u32>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.split(':').map(|x| x.parse::<u32>().ok());
    let (h, mi) = (t.next()??, t.next()??);
    let sec = t.next().flatten().unwrap_or(0);
    ((1..=12).contains(&mo) && (1..=31).contains(&da) && h < 24 && mi < 60)
        .then(|| unix_from_utc(y as i32, mo, da, h, mi, sec))
}

/// "PT15M", "PT60M", "PT1H" → seconds.
fn parse_resolution(s: &str) -> Option<i64> {
    let s = s.trim().strip_prefix("PT")?;
    if let Some(m) = s.strip_suffix('M') {
        return m.parse::<i64>().ok().filter(|&m| m > 0).map(|m| m * 60);
    }
    s.strip_suffix('H')?.parse::<i64>().ok().filter(|&h| h > 0).map(|h| h * 3600)
}

/// Keeps the finest resolution where series overlap (a response may carry
/// the same day in 15-minute and hourly products).
fn merge_finest(mut points: Vec<PricePoint>) -> Vec<PricePoint> {
    points.sort_by_key(|p| (p.dur_s, p.start_s));
    let mut kept: Vec<PricePoint> = Vec::with_capacity(points.len());
    for p in points {
        if !kept.iter().any(|k| p.start_s < k.start_s + k.dur_s && k.start_s < p.start_s + p.dur_s) {
            kept.push(p);
        }
    }
    kept.sort_by_key(|p| p.start_s);
    kept
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| c.tag_name().name() == name)
}

fn text<'a>(n: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(n, name).and_then(|c| c.text()).map(str::trim)
}

/// Parses an ENTSO-E day-ahead price document (A44). With curve type A03
/// a missing position repeats the previous price.
pub fn parse_entsoe(xml: &str) -> Result<Vec<PricePoint>, String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| format!("ENTSO-E: not an XML document ({e})"))?;
    let root = doc.root_element();
    match root.tag_name().name() {
        "Publication_MarketDocument" => {}
        "Acknowledgement_MarketDocument" => {
            let reason = child(root, "Reason").and_then(|r| text(r, "text")).unwrap_or("no reason given");
            return Err(format!("ENTSO-E: {reason}"));
        }
        other => return Err(format!("ENTSO-E: unexpected document {other}")),
    }
    let bad = |what: &str| format!("ENTSO-E: {what}");
    let mut points = Vec::new();
    for ts in root.children().filter(|n| n.tag_name().name() == "TimeSeries") {
        let a03 = text(ts, "curveType") == Some("A03");
        for period in ts.children().filter(|n| n.tag_name().name() == "Period") {
            let interval = child(period, "timeInterval").ok_or_else(|| bad("period without timeInterval"))?;
            let start = text(interval, "start").and_then(parse_utc).ok_or_else(|| bad("bad period start"))?;
            let end = text(interval, "end").and_then(parse_utc).ok_or_else(|| bad("bad period end"))?;
            let res = text(period, "resolution").and_then(parse_resolution).ok_or_else(|| bad("bad resolution"))?;
            let mut given = BTreeMap::new();
            for pt in period.children().filter(|n| n.tag_name().name() == "Point") {
                let pos: i64 = text(pt, "position").and_then(|p| p.parse().ok()).ok_or_else(|| bad("bad position"))?;
                let price: f64 =
                    text(pt, "price.amount").and_then(|p| p.parse().ok()).ok_or_else(|| bad("bad price"))?;
                given.insert(pos, price);
            }
            let mut last = None;
            for pos in 1..=(end - start) / res {
                let v = given.get(&pos).copied().or(if a03 { last } else { None });
                if let Some(v) = v {
                    points.push(PricePoint { start_s: start + (pos - 1) * res, dur_s: res, eur_mwh: v });
                    last = Some(v);
                }
            }
        }
    }
    if points.is_empty() {
        return Err(bad("no prices in the document"));
    }
    Ok(merge_finest(points))
}

/// Durations from the spacing of start times; the last repeats the one before.
fn with_durations(starts_prices: Vec<(i64, f64)>) -> Vec<PricePoint> {
    let mut out: Vec<PricePoint> = Vec::with_capacity(starts_prices.len());
    for (i, &(start, price)) in starts_prices.iter().enumerate() {
        let dur = match starts_prices.get(i + 1) {
            Some(&(next, _)) => next - start,
            None => out.last().map_or(3600, |p| p.dur_s),
        };
        if dur > 0 && dur <= 3600 {
            out.push(PricePoint { start_s: start, dur_s: dur, eur_mwh: price });
        }
    }
    out
}

/// Parses Energy-Charts' `/price` answer: `unix_seconds` (start of each
/// product) and `price` (€/MWh), with gaps as `null`.
pub fn parse_energy_charts(json: &str) -> Result<Vec<PricePoint>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Energy-Charts: not JSON ({e})"))?;
    let (Some(t), Some(p)) = (v["unix_seconds"].as_array(), v["price"].as_array()) else {
        return Err(format!("Energy-Charts: unexpected answer {}", truncate(json)));
    };
    let pairs: Vec<(i64, f64)> = t.iter().zip(p).filter_map(|(t, p)| Some((t.as_i64()?, p.as_f64()?))).collect();
    if pairs.is_empty() {
        return Err("Energy-Charts: no prices in the answer".into());
    }
    Ok(with_durations(pairs))
}

/// Parses `start,eur_mwh` lines; `start` is Unix seconds or UTC like
/// `2025-01-20T16:00Z`. Blank lines, `#` comments and a header are skipped.
pub fn parse_csv(text: &str) -> Result<Vec<PricePoint>, String> {
    let mut pairs = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((a, b)) = line.split_once(',') else { return Err(format!("prices file line {}: no comma", i + 1)) };
        let start = a.trim().parse::<i64>().ok().or_else(|| parse_utc(a));
        match (start, b.trim().parse::<f64>()) {
            (Some(s), Ok(p)) => pairs.push((s, p)),
            _ if i == 0 => continue, // header
            _ => return Err(format!("prices file line {}: expected `start,eur_mwh`", i + 1)),
        }
    }
    pairs.sort_by_key(|&(s, _)| s);
    if pairs.is_empty() {
        return Err("prices file: no prices".into());
    }
    Ok(with_durations(pairs))
}

fn truncate(s: &str) -> String {
    s.chars().take(120).collect()
}

fn yyyymmddhhmm(unix_s: i64) -> String {
    let c = planning::civil_from_unix(unix_s as f64);
    let sod = unix_s.rem_euclid(86_400);
    format!("{:04}{:02}{:02}{:02}{:02}", c.year, c.month, c.day, sod / 3600, sod % 3600 / 60)
}

/// ENTSO-E A44 query for the day-ahead coupling price of `zone`. DE-LU and
/// AT also have a second, local auction; classification sequence 1 selects
/// the SDAC price.
pub fn entsoe_url(token: &str, zone: &str, from_s: i64, to_s: i64) -> Option<String> {
    let code = eic(zone)?;
    let hour = |t: i64| t.div_euclid(3600) * 3600;
    let mut url = format!(
        "{ENTSOE_API}?securityToken={token}&documentType=A44&in_Domain={code}&out_Domain={code}\
         &contract_MarketAgreement.type=A01&periodStart={}&periodEnd={}",
        yyyymmddhhmm(hour(from_s)),
        yyyymmddhhmm(hour(to_s + 3599)),
    );
    if matches!(zone, "DE-LU" | "AT") {
        url.push_str("&classificationSequence_AttributeInstanceComponent.position=1");
    }
    Some(url)
}

pub fn energy_charts_url(zone: &str, from_s: i64, to_s: i64) -> String {
    format!("{ENERGY_CHARTS_API}?bzn={zone}&start={from_s}&end={to_s}")
}

/// Blocking GET with a timeout; the body is returned whatever the status,
/// because ENTSO-E explains its errors in the body.
pub fn http_get(url: &str) -> Result<(u16, String), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .user_agent(concat!("grid-edge-gateway/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    Ok((status, body))
}

/// What to fetch, taken out of the planner's lock.
#[derive(Debug, Clone)]
pub struct PriceJob {
    pub sources: Vec<String>,
    pub zone: String,
    pub file: Option<std::path::PathBuf>,
    pub from_s: i64,
    pub to_s: i64,
}

fn fetch_one(source: &str, job: &PriceJob) -> Result<Vec<PricePoint>, String> {
    match source {
        "entsoe" => {
            let token = std::env::var("ENTSOE_TOKEN")
                .ok()
                .filter(|t| !t.trim().is_empty())
                .ok_or("ENTSO-E: no security token (set ENTSOE_TOKEN)")?;
            let url = entsoe_url(token.trim(), &job.zone, job.from_s, job.to_s)
                .ok_or_else(|| format!("ENTSO-E: unknown bidding zone {}", job.zone))?;
            // Never let the token reach a log line.
            let (status, body) = http_get(&url).map_err(|e| format!("ENTSO-E: {}", e.replace(token.trim(), "***")))?;
            match parse_entsoe(&body) {
                Ok(p) => Ok(p),
                Err(e) if status != 200 => Err(format!("{e} (HTTP {status})")),
                Err(e) => Err(e),
            }
        }
        "energy-charts" => {
            let (status, body) = http_get(&energy_charts_url(&job.zone, job.from_s, job.to_s))
                .map_err(|e| format!("Energy-Charts: {e}"))?;
            if status != 200 {
                return Err(format!("Energy-Charts: HTTP {status}: {}", truncate(&body)));
            }
            parse_energy_charts(&body)
        }
        "file" => {
            let path = job.file.as_ref().ok_or("prices: source \"file\" needs `file`")?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            parse_csv(&text)
        }
        other => Err(format!("unknown price source {other:?}")),
    }
}

/// Prices from the first source that answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched {
    pub source: String,
    pub points: Vec<PricePoint>,
    /// Why the sources before it were skipped.
    pub skipped: Vec<String>,
}

/// Tries the sources in order. Returns the first that answers, or every
/// source's error.
pub fn fetch(job: &PriceJob) -> Result<Fetched, String> {
    let mut errors = Vec::new();
    for s in &job.sources {
        match fetch_one(s, job) {
            Ok(points) => return Ok(Fetched { source: s.clone(), points, skipped: errors }),
            Err(e) => errors.push(e),
        }
    }
    Err(errors.join("; "))
}

/// The prices known to the gateway, newest answer winning where they overlap.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PriceBook {
    points: Vec<PricePoint>,
}

impl PriceBook {
    pub fn points(&self) -> &[PricePoint] {
        &self.points
    }

    /// Adds fresh prices, replacing what they overlap, and forgets prices
    /// older than a week before `now_s`.
    pub fn merge(&mut self, fresh: Vec<PricePoint>, now_s: i64) {
        let Some(lo) = fresh.first().map(|p| p.start_s) else { return };
        let hi = fresh.last().map_or(lo, |p| p.start_s + p.dur_s);
        self.points.retain(|p| (p.start_s + p.dur_s <= lo || p.start_s >= hi) && p.start_s > now_s - 8 * 86_400);
        self.points.extend(fresh);
        self.points.sort_by_key(|p| p.start_s);
    }

    /// The price of the product containing `t_s`.
    pub fn at(&self, t_s: i64) -> Option<f64> {
        let i = self.points.partition_point(|p| p.start_s <= t_s);
        let p = self.points.get(i.checked_sub(1)?)?;
        (t_s < p.start_s + p.dur_s).then_some(p.eur_mwh)
    }

    /// The price at `t_s`, or, where it is not published yet, the price at
    /// the same time one, two or seven days earlier. `bool`: published.
    pub fn at_or_before(&self, t_s: i64) -> Option<(f64, bool)> {
        if let Some(p) = self.at(t_s) {
            return Some((p, true));
        }
        [1, 2, 7].iter().find_map(|d| self.at(t_s - d * 86_400)).map(|p| (p, false))
    }

    /// End of the last published price.
    pub fn until_s(&self) -> Option<i64> {
        self.points.last().map(|p| p.start_s + p.dur_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of an A44 answer, with curve type A03: positions 3 and 4
    /// are left out because they repeat position 2.
    const A44: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Publication_MarketDocument xmlns="urn:iec62325.351:tc57wg16:451-3:publicationdocument:7:3">
  <mRID>6d2f</mRID>
  <type>A44</type>
  <period.timeInterval><start>2025-10-05T22:00Z</start><end>2025-10-05T23:30Z</end></period.timeInterval>
  <TimeSeries>
    <mRID>1</mRID>
    <auction.type>A01</auction.type>
    <businessType>A62</businessType>
    <in_Domain.mRID codingScheme="A01">10Y1001A1001A82H</in_Domain.mRID>
    <out_Domain.mRID codingScheme="A01">10Y1001A1001A82H</out_Domain.mRID>
    <contract_MarketAgreement.type>A01</contract_MarketAgreement.type>
    <currency_Unit.name>EUR</currency_Unit.name>
    <price_Measure_Unit.name>MWH</price_Measure_Unit.name>
    <classificationSequence_AttributeInstanceComponent.position>1</classificationSequence_AttributeInstanceComponent.position>
    <curveType>A03</curveType>
    <Period>
      <timeInterval><start>2025-10-05T22:00Z</start><end>2025-10-05T23:30Z</end></timeInterval>
      <resolution>PT15M</resolution>
      <Point><position>1</position><price.amount>91.02</price.amount></Point>
      <Point><position>2</position><price.amount>85.5</price.amount></Point>
      <Point><position>5</position><price.amount>-3.1</price.amount></Point>
      <Point><position>6</position><price.amount>0</price.amount></Point>
    </Period>
  </TimeSeries>
  <TimeSeries>
    <mRID>2</mRID>
    <curveType>A01</curveType>
    <Period>
      <timeInterval><start>2025-10-05T22:00Z</start><end>2025-10-05T23:00Z</end></timeInterval>
      <resolution>PT60M</resolution>
      <Point><position>1</position><price.amount>80.0</price.amount></Point>
    </Period>
  </TimeSeries>
</Publication_MarketDocument>"#;

    #[test]
    fn entsoe_a03_repeats_left_out_positions_and_prefers_quarter_hours() {
        let p = parse_entsoe(A44).unwrap();
        let t0 = unix_from_utc(2025, 10, 5, 22, 0, 0);
        let prices: Vec<f64> = p.iter().map(|x| x.eur_mwh).collect();
        assert_eq!(prices, [91.02, 85.5, 85.5, 85.5, -3.1, 0.0], "the hourly product is covered by the 15-min ones");
        assert_eq!(p[0].start_s, t0);
        assert!(p.iter().all(|x| x.dur_s == 900));
        assert_eq!(p[5].start_s, t0 + 5 * 900);
    }

    #[test]
    fn entsoe_errors_are_reported_with_their_reason() {
        let ack = r#"<?xml version="1.0" encoding="UTF-8"?>
<Acknowledgement_MarketDocument xmlns="urn:iec62325.351:tc57wg16:451-1:acknowledgementdocument:7:0">
  <mRID>613ca134</mRID>
  <Reason><code>999</code><text>Authentication failed.</text></Reason>
</Acknowledgement_MarketDocument>"#;
        assert_eq!(parse_entsoe(ack).unwrap_err(), "ENTSO-E: Authentication failed.");
        assert!(parse_entsoe("<html>busy</html>").unwrap_err().contains("unexpected document"));
        assert!(parse_entsoe("not xml").is_err());
    }

    #[test]
    fn entsoe_query_selects_the_sdac_auction_and_whole_hours() {
        let from = unix_from_utc(2026, 9, 23, 22, 10, 0);
        let url = entsoe_url("TOKEN", "DE-LU", from, from + 86_400).unwrap();
        assert!(url.contains("documentType=A44&in_Domain=10Y1001A1001A82H&out_Domain=10Y1001A1001A82H"));
        assert!(url.contains("periodStart=202609232200&periodEnd=202609242300"));
        assert!(url.contains("classificationSequence_AttributeInstanceComponent.position=1"));
        assert!(!entsoe_url("T", "CH", from, from).unwrap().contains("classificationSequence"));
        assert!(entsoe_url("T", "FR", from, from).is_none());
    }

    #[test]
    fn energy_charts_answer_with_quarter_hours_hours_and_gaps() {
        let json = r#"{"license_info":"CC BY 4.0","unix_seconds":[1790200800,1790201700,1790202600,1790203500],
                       "price":[88.1,null,70.0,65.5],"unit":"EUR / MWh","deprecated":false}"#;
        let p = parse_energy_charts(json).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!((p[0].start_s, p[0].dur_s), (1_790_200_800, 1800), "the gap stretches the product before it");
        assert_eq!(p[2].dur_s, 900);
        let hourly = r#"{"unix_seconds":[1790190000,1790193600],"price":[100.0,90.0]}"#;
        assert!(parse_energy_charts(hourly).unwrap().iter().all(|p| p.dur_s == 3600));
        assert!(parse_energy_charts(r#"{"detail":"bzn not found"}"#).is_err());
    }

    #[test]
    fn csv_takes_unix_seconds_or_utc_times() {
        let p = parse_csv("start,eur_mwh\n# note\n2025-01-20T16:00Z,402.12\n1737392400,583.4\n").unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].start_s, unix_from_utc(2025, 1, 20, 16, 0, 0));
        assert_eq!(p[0].dur_s, 3600);
        assert!(parse_csv("1,2\nx;y\n").is_err());
    }

    #[test]
    fn book_looks_up_merges_and_falls_back_to_earlier_days() {
        let day = 86_400;
        let t0 = unix_from_utc(2026, 9, 24, 0, 0, 0);
        let mut b = PriceBook::default();
        b.merge((0..96).map(|k| PricePoint { start_s: t0 + k * 900, dur_s: 900, eur_mwh: k as f64 }).collect(), t0);
        assert_eq!(b.at(t0 + 901), Some(1.0));
        assert_eq!(b.at(t0 + day), None);
        assert_eq!(b.at_or_before(t0 + day + 901), Some((1.0, false)), "tomorrow not published: yesterday's");
        assert_eq!(b.until_s(), Some(t0 + day));
        // a correction replaces what it overlaps
        b.merge(vec![PricePoint { start_s: t0 + 900, dur_s: 900, eur_mwh: 50.0 }], t0);
        assert_eq!(b.at(t0 + 901), Some(50.0));
        assert_eq!(b.at(t0), Some(0.0));
        assert_eq!(b.points().len(), 96);
        // old prices are forgotten
        b.merge(vec![PricePoint { start_s: t0 + 10 * day, dur_s: 900, eur_mwh: 1.0 }], t0 + 10 * day);
        assert_eq!(b.points().len(), 1);
    }
}
