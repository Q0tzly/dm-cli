use anyhow::{Context, Result, bail};
use chrono::Duration;

pub fn parse_age(input: &str) -> Result<Duration> {
    let trimmed = input.trim();
    if trimmed.len() < 2 {
        bail!("duration must include a number and unit, for example 14d");
    }

    let (number, unit) = trimmed.split_at(trimmed.len() - 1);
    let value: i64 = number
        .parse()
        .with_context(|| format!("invalid duration value {number:?}"))?;

    if value < 0 {
        bail!("duration must not be negative");
    }

    match unit {
        "h" => Ok(Duration::hours(value)),
        "d" => Ok(Duration::days(value)),
        "w" => Ok(Duration::weeks(value)),
        "m" => Ok(Duration::days(value * 30)),
        u if u.chars().all(|c| c.is_ascii_digit()) => {
            bail!("missing unit suffix (expected d, w, or m); did you mean \"{u}d\"?")
        }
        _ => bail!("unsupported duration unit {unit:?}; use h, d, w, or m"),
    }
}

pub fn human_days_since(then: chrono::DateTime<chrono::Utc>) -> i64 {
    chrono::Utc::now()
        .signed_duration_since(then)
        .num_days()
        .max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_age_units() {
        assert_eq!(parse_age("14d").unwrap(), Duration::days(14));
        assert_eq!(parse_age("24h").unwrap(), Duration::hours(24));
        assert_eq!(parse_age("2w").unwrap(), Duration::days(14));
        assert_eq!(parse_age("1m").unwrap(), Duration::days(30));
    }

    #[test]
    fn rejects_invalid_age_values() {
        assert!(parse_age("d").is_err());
        assert!(parse_age("-1d").is_err());
        assert!(parse_age("14x").is_err());
    }
}
