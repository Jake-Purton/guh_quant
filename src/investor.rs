use serde::Deserialize;
use std::error::Error;

#[derive(Debug, Deserialize)]
pub struct ContextResponse {
    pub message: String,
}

#[derive(Debug)]
pub struct InvestorProfile {
    pub name: String,
    pub age: u32,
    pub budget: f64,
    pub excluded_sectors: Vec<String>,
    pub risk_tolerance: RiskLevel,
    pub start_year: Option<u32>,
    pub end_year: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub enum RiskLevel {
    Conservative,  // Age 60+: 25% stocks
    Moderate,      // Age 40-59: 65% stocks  
    Aggressive,    // Age <40: 85% stocks
}

impl InvestorProfile {
    pub fn from_context(context_json: &str) -> Result<Self, Box<dyn Error>> {
        let ctx: ContextResponse = serde_json::from_str(context_json)?;
        let msg = &ctx.message;
        let msg_lower = msg.to_lowercase();

        // Extract age - pattern: "X-year-old" or "X years old"
        // If no age is provided, default to 45 (moderate risk)
        let age = Self::extract_number(&msg_lower, r"(\d+)-year-old")
            .or_else(|| Self::extract_number(&msg_lower, r"(\d+)\s+years?\s+old"))
            .unwrap_or(45);

        // Extract budget - must explicitly mention "budget"
        let budget = Self::extract_money(&msg_lower, r"budget of \$([0-9,]+)")
            .or_else(|| Self::extract_money(&msg_lower, r"budget is \$([0-9,]+)"))
            .or_else(|| Self::extract_money(&msg_lower, r"total budget of \$([0-9,]+)"))
            .or_else(|| Self::extract_money(&msg_lower, r"investment is \$([0-9,]+)"))
            .ok_or("Budget not found in context — must include 'budget of' or 'budget is'")?;


        // Extract name (first two capitalized words)
        let name = msg
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");

        // Extract excluded sectors
        let excluded_sectors = Self::extract_excluded_sectors(&msg_lower);

        // Extract investment dates
        // Try multiple patterns to catch "start date is 2008-08-22" or "start 2008"
        let start_year = Self::extract_year(&msg_lower, r"start.*?date.*?(\d{4})")
            .or_else(|| Self::extract_year(&msg_lower, r"start.*?(\d{4})"))
            .or_else(|| Self::extract_year(msg, r"(?:january|february|march|april|may|june|july|august|september|october|november|december).*?(\d{4})"));
        let end_year = Self::extract_year(&msg_lower, r"end.*?date.*?(\d{4})")
            .or_else(|| Self::extract_year(&msg_lower, r"end.*?(\d{4})"));

        // Determine risk level. First prefer an explicit mention in the brief
        // (e.g. "conservative", "moderate", "aggressive", "risk averse").
        // If none found, fall back to an age-based heuristic.
        let explicit_risk = {
            if msg_lower.contains("conservative") || msg_lower.contains("risk averse") || msg_lower.contains("risk-averse") || msg_lower.contains("low risk") {
                Some(RiskLevel::Conservative)
            } else if msg_lower.contains("aggressive") || msg_lower.contains("high risk") || msg_lower.contains("very aggressive") || msg_lower.contains("risk seeking") || msg_lower.contains("risk-seeking") {
                Some(RiskLevel::Aggressive)
            } else if msg_lower.contains("moderate") || msg_lower.contains("balanced") || msg_lower.contains("medium risk") || msg_lower.contains("moderately aggressive") {
                Some(RiskLevel::Moderate)
            } else {
                None
            }
        };

        let risk_tolerance = explicit_risk.unwrap_or_else(|| match age {
            0..=39 => RiskLevel::Aggressive,
            40..=59 => RiskLevel::Moderate,
            _ => RiskLevel::Conservative,
        });

        Ok(InvestorProfile {
            name,
            age,
            budget,
            excluded_sectors,
            risk_tolerance,
            start_year,
            end_year,
        })
    }

    fn extract_number(text: &str, pattern: &str) -> Option<u32> {
        regex::Regex::new(pattern)
            .ok()?
            .captures(text)?
            .get(1)?
            .as_str()
            .parse()
            .ok()
    }

    fn extract_money(text: &str, pattern: &str) -> Option<f64> {
        regex::Regex::new(pattern)
            .ok()?
            .captures(text)?
            .get(1)?
            .as_str()
            .replace(",", "")
            .parse()
            .ok()
    }

    fn extract_year(text: &str, pattern: &str) -> Option<u32> {
        regex::Regex::new(pattern)
            .ok()?
            .captures(text)?
            .get(1)?
            .as_str()
            .parse()
            .ok()
    }

    fn extract_excluded_sectors(text: &str) -> Vec<String> {
        use regex::Regex;
        let mut sectors = Vec::new();

        // Capture after the word 'avoid' or 'avoids' until end of sentence/newline
        // and split into tokens using commas, semicolons, 'and' and 'or'.
        if let Ok(re) = Regex::new(r"avoids?\s+([^\.\n]+)") {
            if let Some(cap) = re.captures(text) {
                if let Some(m) = cap.get(1) {
                    let raw = m.as_str();
                    for part in raw.split(|c: char| c == ',' || c == ';') {
                        for sub in part.split(" and ").flat_map(|s| s.split(" or ")) {
                            let token = sub
                                .trim()
                                .trim_end_matches('.')
                                .to_lowercase();
                            if !token.is_empty() {
                                // map token to canonical sector(s)
                                let mut matched = false;
                                // Broad mapping of substrings to canonical sector names
                                let mapping = [
                                    ("crypto assets", "Crypto"), ("crypto asset", "Crypto"), ("crypto", "Crypto"), ("cryptocurrency", "Crypto"), ("blockchain", "Crypto"), ("bitcoin", "Crypto"),
                                    ("real estate", "Real Estate"), ("reit", "Real Estate"), ("property", "Real Estate"),
                                    ("construction", "Construction"),
                                    ("industrial applications and services", "Industrials"), ("industrial applications", "Industrials"), ("industrial apps", "Industrials"), ("industrial", "Industrials"), ("manufacturing", "Manufacturing"), ("manufactur", "Manufacturing"),
                                    ("industrials", "Industrials"),
                                    ("technology", "Technology"), ("tech", "Technology"), ("software", "Technology"), ("semiconductor", "Technology"), ("semiconductors", "Technology"), ("chip", "Technology"), ("hardware", "Technology"), ("internet", "Technology"), ("e-commerce", "Technology"), ("ecommerce", "Technology"), ("cloud", "Technology"), ("platform", "Technology"), ("ai", "Technology"),
                                    ("life sciences", "Healthcare"), ("life-sciences", "Healthcare"), ("healthcare", "Healthcare"), ("health", "Healthcare"), ("pharmaceutical", "Healthcare"), ("pharma", "Healthcare"), ("biotech", "Healthcare"),
                                    ("financials", "Financials"), ("finance", "Financials"), ("bank", "Financials"), ("banking", "Financials"), ("insurance", "Financials"), ("investment", "Financials"), ("structured finance", "Financials"), ("international corp fin", "Financials"), ("manufactured finance", "Financials"),
                                    ("energy", "Energy"), ("oil", "Energy"), ("gas", "Energy"), ("petroleum", "Energy"), ("renewable", "Energy"),
                                    ("transportation", "Transportation"), ("transport", "Transportation"), ("shipping", "Transportation"),
                                    ("utilities", "Utilities"), ("utility", "Utilities"), ("electric", "Utilities"), ("power", "Utilities"),
                                    ("consumer", "Consumer"), ("retail", "Consumer"), ("restaurant", "Consumer"), ("food", "Consumer"), ("beverage", "Consumer"),
                                    ("trade and services", "Industrials"),
                                ];

                                for (pat, canon) in &mapping {
                                    if token.contains(pat) {
                                        if !sectors.contains(&canon.to_string()) {
                                            sectors.push(canon.to_string());
                                        }
                                        matched = true;
                                    }
                                }

                                // If no mapping matched, try some heuristics: single words like 'crypto', 'tech', etc.
                                if !matched {
                                    let heur = [
                                        ("crypto", "Crypto"), ("tech", "Technology"), ("software", "Technology"), ("manufactur", "Manufacturing"), ("industrial", "Industrials"), ("finance", "Financials"), ("health", "Healthcare"), ("energy", "Energy"), ("transport", "Transportation"), ("real estate", "Real Estate"),
                                    ];
                                    for (pat, canon) in &heur {
                                        if token.contains(pat) {
                                            if !sectors.contains(&canon.to_string()) {
                                                sectors.push(canon.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // As a safety-net, also scan the whole text for obvious keywords that
        // might indicate exclusions even if the 'avoid' capture failed.
        let global_mapping = [
            ("industrial applications", "Industrials"), ("industrial", "Industrials"), ("manufactur", "Manufacturing"),
            ("technology", "Technology"), ("tech", "Technology"), ("software", "Technology"), ("semiconductor", "Technology"),
            ("crypto", "Crypto"), ("real estate", "Real Estate"), ("construction", "Construction"), ("healthcare", "Healthcare"), ("finance", "Financials"), ("energy", "Energy"),
        ];
        for (pat, canon) in &global_mapping {
            if text.contains(pat) {
                if !sectors.contains(&canon.to_string()) {
                    sectors.push(canon.to_string());
                }
            }
        }

        sectors
    }

    pub fn should_exclude_sector(&self, sector: &str) -> bool {
        self.excluded_sectors
            .iter()
            .any(|s| s.eq_ignore_ascii_case(sector))
    }

    /// Extended exclusion check: matches by exact sector, substrings, stock name,
    /// and a small synonym map so "Technology" will match "Software", "Internet",
    /// "Semiconductors", etc. This is conservative: if any excluded term appears
    /// in the stock sector or name we treat it as excluded.
    pub fn should_exclude_sector_extended(&self, sector: &str, stock_name: &str) -> bool {
        if self.excluded_sectors.is_empty() {
            return false;
        }

        let sector_low = sector.to_ascii_lowercase();
        let name_low = stock_name.to_ascii_lowercase();

        for ex in &self.excluded_sectors {
            let ex_low = ex.to_ascii_lowercase();

            // Exact match or case-insensitive equality
            if sector_low == ex_low || ex_low == name_low {
                return true;
            }

            // Substring match in sector or stock name
            if sector_low.contains(&ex_low) || name_low.contains(&ex_low) {
                return true;
            }

            // Small synonyms map for common sector aliases
            match ex_low.as_str() {
                "technology" | "tech" => {
                    if sector_low.contains("software")
                        || sector_low.contains("semicon")
                        || sector_low.contains("semiconductor")
                        || sector_low.contains("internet")
                        || sector_low.contains("hardware")
                        || sector_low.contains("electronic")
                        || sector_low.contains("cloud")
                        || sector_low.contains("e-comm")
                        || sector_low.contains("ecom")
                        || sector_low.contains("platform")
                        || name_low.contains("tech")
                        || name_low.contains("cloud")
                    {
                        return true;
                    }
                }
                "manufacturing" | "manufactur" | "industrials" => {
                    if sector_low.contains("industrial")
                        || sector_low.contains("manufactur")
                        || sector_low.contains("applications")
                        || name_low.contains("industrial")
                    {
                        return true;
                    }
                }
                "crypto" | "crypto assets" | "cryptocurrency" => {
                    if sector_low.contains("crypto")
                        || sector_low.contains("blockchain")
                        || sector_low.contains("coin")
                        || name_low.contains("coin")
                    {
                        return true;
                    }
                }
                "financials" | "finance" => {
                    if sector_low.contains("bank")
                        || sector_low.contains("finance")
                        || sector_low.contains("insurance")
                        || sector_low.contains("investment")
                    {
                        return true;
                    }
                }
                "healthcare" | "life sciences" => {
                    if sector_low.contains("health")
                        || sector_low.contains("pharma")
                        || sector_low.contains("biotech")
                    {
                        return true;
                    }
                }
                "energy" => {
                    if sector_low.contains("oil") || sector_low.contains("gas") || sector_low.contains("energy") {
                        return true;
                    }
                }
                _ => {}
            }
        }

        false
    }
}
