//! What the grid operator may do, per country.
//!
//! The mechanics (a DSO command limits consumption or feed-in) are the same
//! across Central Europe; the limits differ. A [`Policy`] captures them so
//! the controller stays country-agnostic.
//!
//! | | Consumption | Feed-in |
//! |---|---|---|
//! | DE | §14a EnWG: dim steuVE to Pmin,14a (BNetzA BK6-22-300) | DSO setpoint (EEG §9, plants > 100 kW); new systems without smart meter capped at 60% (Solarspitzengesetz 2025) |
//! | AT | by contract only — no statutory minimum found in the sources checked | DSO may cap feed-in of new/extended PV at up to 70% of module peak power (ElWG "Spitzenkappung"); dynamic version from 2028 |
//! | CH | by contract, the owner may forbid existing uses (StromVG Art. 17c, StromVV Art. 19b/19d) | guaranteed, unpaid curtailment of at most 3% of the energy produced per year at the connection point (StromVV Art. 19c Abs. 4); unlimited in an immediate, serious threat (Art. 17c Abs. 4b) |

/// Countries with a ready-made policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jurisdiction {
    De,
    At,
    Ch,
}

impl Jurisdiction {
    pub fn code(self) -> &'static str {
        match self {
            Jurisdiction::De => "DE",
            Jurisdiction::At => "AT",
            Jurisdiction::Ch => "CH",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "DE" => Some(Jurisdiction::De),
            "AT" => Some(Jurisdiction::At),
            "CH" => Some(Jurisdiction::Ch),
            _ => None,
        }
    }
}

/// How a consumption-reduction command is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConsumptionRule {
    /// §14a EnWG via an EMS: controllable devices may still draw Pmin,14a from
    /// the grid (computed from the site's devices), PV surplus on top.
    De14a,
    /// A flexibility contract: the devices may draw at least `min_kw` from the
    /// grid, and the DSO may use the contract at most `max_minutes_per_day`
    /// (0 = no daily limit). Devices whose owner opted out are left alone.
    Contract { min_kw: f64, max_minutes_per_day: f64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    pub jurisdiction: Jurisdiction,
    pub consumption: ConsumptionRule,
    /// A standing cap on feed-in at the grid connection, % of installed PV
    /// (DE Solarspitzengesetz 60, AT Spitzenkappung up to 70). `None` = none.
    pub static_feed_in_cap_pct: Option<f64>,
    /// Curtailment the DSO may impose for free, as a share of the energy the
    /// PV produces in a calendar year (CH: 3). `None` = no such budget.
    pub curtailment_budget_pct: Option<f64>,
    /// Refuse non-emergency curtailment once the budget is used up. When
    /// false the gateway only reports it.
    pub enforce_curtailment_budget: bool,
}

impl Policy {
    /// The default for a country. Site-specific values (the AT cap the DSO
    /// chose, a CH contract) are set in the gateway configuration.
    pub fn for_country(j: Jurisdiction) -> Self {
        match j {
            Jurisdiction::De => Policy {
                jurisdiction: j,
                consumption: ConsumptionRule::De14a,
                static_feed_in_cap_pct: None,
                curtailment_budget_pct: None,
                enforce_curtailment_budget: false,
            },
            Jurisdiction::At => Policy {
                jurisdiction: j,
                consumption: ConsumptionRule::Contract { min_kw: 0.0, max_minutes_per_day: 0.0 },
                static_feed_in_cap_pct: Some(70.0),
                curtailment_budget_pct: None,
                enforce_curtailment_budget: false,
            },
            Jurisdiction::Ch => Policy {
                jurisdiction: j,
                consumption: ConsumptionRule::Contract { min_kw: 0.0, max_minutes_per_day: 0.0 },
                static_feed_in_cap_pct: None,
                curtailment_budget_pct: Some(3.0),
                enforce_curtailment_budget: true,
            },
        }
    }

    /// Checks values a configuration file could get wrong.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(c) = self.static_feed_in_cap_pct
            && !(0.0..=100.0).contains(&c)
        {
            return Err(format!("static feed-in cap {c}% is outside 0–100%"));
        }
        if let Some(b) = self.curtailment_budget_pct
            && !(0.0..=100.0).contains(&b)
        {
            return Err(format!("curtailment budget {b}% is outside 0–100%"));
        }
        if let ConsumptionRule::Contract { min_kw, max_minutes_per_day } = self.consumption
            && (min_kw < 0.0 || max_minutes_per_day < 0.0 || max_minutes_per_day > 1440.0)
        {
            return Err("contract: min_kw must be ≥ 0 and max_minutes_per_day within 0–1440".into());
        }
        if self.jurisdiction == Jurisdiction::De && self.consumption != ConsumptionRule::De14a {
            return Err("DE sites are dimmed under §14a EnWG; a contract rule does not apply".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn country_defaults_are_valid() {
        for j in [Jurisdiction::De, Jurisdiction::At, Jurisdiction::Ch] {
            Policy::for_country(j).validate().unwrap();
            assert_eq!(Jurisdiction::parse(j.code()), Some(j));
        }
        assert_eq!(Policy::for_country(Jurisdiction::Ch).curtailment_budget_pct, Some(3.0));
        assert_eq!(Policy::for_country(Jurisdiction::At).static_feed_in_cap_pct, Some(70.0));
    }

    #[test]
    fn rejects_nonsense() {
        let mut p = Policy::for_country(Jurisdiction::At);
        p.static_feed_in_cap_pct = Some(140.0);
        assert!(p.validate().is_err());
        let mut p = Policy::for_country(Jurisdiction::De);
        p.consumption = ConsumptionRule::Contract { min_kw: 5.0, max_minutes_per_day: 60.0 };
        assert!(p.validate().is_err());
    }
}
