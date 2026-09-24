//! Human-readable one-line descriptions of APDUs, for logs and the dashboard.

use crate::apci::{Apdu, UFunction};
use crate::asdu::{Asdu, AsduError, Cause, Element};

pub fn describe(frame: &[u8]) -> String {
    match Apdu::parse(frame) {
        Ok(Some((Apdu::U(f), _))) => format!("U {}", u_name(f)),
        Ok(Some((Apdu::S { nr }, _))) => format!("S nr={nr}"),
        Ok(Some((Apdu::I { ns, nr, asdu }, _))) => format!("I ns={ns} nr={nr} {}", describe_asdu(&asdu)),
        Ok(None) => "incomplete frame".into(),
        Err(e) => format!("invalid frame: {e}"),
    }
}

pub fn describe_asdu(raw: &[u8]) -> String {
    match Asdu::decode(raw) {
        Ok(a) => {
            let mut s = format!(
                "{} {}{} CA={}",
                Element::type_name(a.type_id),
                cause_name(a.cause),
                if a.negative { " NEGATIVE" } else { "" },
                a.common_address
            );
            for o in a.objects.iter().take(4) {
                s.push_str(&format!(" | IOA {} {}", o.ioa, element_value(&o.element)));
            }
            if a.objects.len() > 4 {
                s.push_str(&format!(" | +{} more", a.objects.len() - 4));
            }
            s
        }
        Err(AsduError::UnsupportedType { type_id, .. }) => format!("type {type_id} (not supported)"),
        Err(AsduError::UnsupportedCause { .. }) => "unknown cause of transmission".into(),
        Err(AsduError::Truncated) => "truncated ASDU".into(),
    }
}

fn u_name(f: UFunction) -> &'static str {
    match f {
        UFunction::StartDtAct => "STARTDT act",
        UFunction::StartDtCon => "STARTDT con",
        UFunction::StopDtAct => "STOPDT act",
        UFunction::StopDtCon => "STOPDT con",
        UFunction::TestFrAct => "TESTFR act",
        UFunction::TestFrCon => "TESTFR con",
    }
}

pub fn cause_name(c: Cause) -> &'static str {
    match c {
        Cause::Periodic => "per/cyc",
        Cause::Background => "back",
        Cause::Spontaneous => "spont",
        Cause::Initialized => "init",
        Cause::Request => "req",
        Cause::Activation => "act",
        Cause::ActivationCon => "actcon",
        Cause::Deactivation => "deact",
        Cause::DeactivationCon => "deactcon",
        Cause::ActivationTermination => "actterm",
        Cause::Interrogated => "inrogen",
        Cause::UnknownType => "unknown type",
        Cause::UnknownCause => "unknown cause",
        Cause::UnknownCommonAddress => "unknown CA",
        Cause::UnknownObjectAddress => "unknown IOA",
    }
}

fn element_value(e: &Element) -> String {
    match *e {
        Element::SinglePoint { on, .. } | Element::SinglePointTime { on, .. } => (if on { "ON" } else { "OFF" }).into(),
        Element::Float { value, quality } | Element::FloatTime { value, quality, .. } => {
            format!("{value:.2}{}", if quality.invalid { " IV" } else { "" })
        }
        Element::SingleCommand { on, select, .. } => {
            format!("{}{}", if on { "ON" } else { "OFF" }, if select { " (select)" } else { "" })
        }
        Element::SetpointFloat { value, select, .. } => format!("{value:.2}{}", if select { " (select)" } else { "" }),
        Element::Interrogation { qoi } => format!("QOI={qoi}"),
        Element::EndOfInit { coi } => format!("COI={coi}"),
        Element::ClockSync { time } => format!(
            "20{:02}-{:02}-{:02} {:02}:{:02}:{:06.3}",
            time.year,
            time.month,
            time.day,
            time.hour,
            time.minute,
            time.millisecond as f64 / 1000.0
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_a_setpoint_command() {
        let asdu = Asdu::single(Cause::Activation, 1, 5002, Element::SetpointFloat { value: 60.0, select: false, qualifier: 0 });
        let frame = Apdu::I { ns: 3, nr: 5, asdu: asdu.encode() }.encode();
        assert_eq!(describe(&frame), "I ns=3 nr=5 C_SE_NC_1 act CA=1 | IOA 5002 60.00");
        assert_eq!(describe(&Apdu::U(UFunction::StartDtAct).encode()), "U STARTDT act");
    }
}
