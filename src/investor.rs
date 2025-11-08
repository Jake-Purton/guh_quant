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

        // Extract budget - pattern: "budget of $X" or "$X"
        let budget = Self::extract_money(&msg_lower, r"budget of \$([0-9,]+)")
            .or_else(|| Self::extract_money(&msg_lower, r"total budget of \$([0-9,]+)"))
            .or_else(|| Self::extract_money(&msg_lower, r"\$([0-9,]+)"))
            .unwrap_or(1000000000000.0);

        // Extract name (first two capitalized words)
        let name = msg
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");

        // Extract excluded sectors
        let excluded_sectors = Self::extract_excluded_sectors(&msg_lower);

        // Extract investment dates
        let start_year = Self::extract_year(&msg_lower, r"start.*?(\d{4})")
            .or_else(|| Self::extract_year(msg, r"(?:january|february|march|april|may|june|july|august|september|october|november|december).*?(\d{4})"));
        let end_year = Self::extract_year(&msg_lower, r"end.*?(\d{4})");

        // Determine risk level
        let risk_tolerance = match age {
            0..=39 => RiskLevel::Aggressive,
            40..=59 => RiskLevel::Moderate,
            _ => RiskLevel::Conservative,
        };

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
        let mut sectors = Vec::new();
        
        // Look for "avoids" keyword
        if !text.contains("avoids") && !text.contains("avoid") {
            return sectors;
        }

        // Map keywords to standardized sector names
        let sector_map = [
            ("crypto assets", "Crypto"),
            ("crypto", "Crypto"),
            ("cryptocurrency", "Crypto"),
            ("real estate", "Real Estate"),
            ("construction", "Construction"),
            ("manufacturing", "Manufacturing"),
            ("industrials", "Industrials"),
            ("technology", "Technology"),
            ("tech", "Technology"),
            ("healthcare", "Healthcare"),
            ("health", "Healthcare"),
            ("financials", "Financials"),
            ("finance", "Financials"),
            ("banking", "Financials"),
            ("energy", "Energy"),
            ("utilities", "Utilities"),
            ("consumer", "Consumer"),
        ];

        for (keyword, sector) in sector_map {
            if text.contains(keyword) {
                if !sectors.contains(&sector.to_string()) {
                    sectors.push(sector.to_string());
                }
            }
        }

        sectors
    }

    pub fn stock_allocation_pct(&self) -> f64 {
        match self.risk_tolerance {
            RiskLevel::Conservative => 0.25,
            RiskLevel::Moderate => 0.65,
            RiskLevel::Aggressive => 0.85,
        }
    }

    pub fn stock_budget(&self) -> f64 {
        self.budget * self.stock_allocation_pct()
    }

    pub fn should_exclude_sector(&self, sector: &str) -> bool {
        self.excluded_sectors
            .iter()
            .any(|s| s.eq_ignore_ascii_case(sector))
    }
}
