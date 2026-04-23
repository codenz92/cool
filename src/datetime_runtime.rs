use chrono::{DateTime, Datelike, Local, NaiveDate, NaiveDateTime, TimeZone, Timelike};

pub const DEFAULT_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateTimeParts {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub weekday: i64,
    pub yearday: i64,
}

pub fn now() -> f64 {
    let dt = Local::now();
    dt.timestamp() as f64 + dt.timestamp_subsec_nanos() as f64 / 1_000_000_000.0
}

pub fn format(timestamp: f64, fmt: Option<&str>) -> Result<String, String> {
    let dt = local_from_timestamp(timestamp)?;
    Ok(dt.format(normalize_format(fmt)?).to_string())
}

pub fn parse(text: &str, fmt: Option<&str>) -> Result<f64, String> {
    let fmt = normalize_format(fmt)?;

    if let Ok(dt) = DateTime::parse_from_str(text, fmt) {
        return Ok(dt.timestamp() as f64 + dt.timestamp_subsec_nanos() as f64 / 1_000_000_000.0);
    }

    if let Ok(naive) = NaiveDateTime::parse_from_str(text, fmt) {
        let dt = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| format!("datetime.parse() produced an ambiguous local time for '{}'", text))?;
        return Ok(dt.timestamp() as f64 + dt.timestamp_subsec_nanos() as f64 / 1_000_000_000.0);
    }

    if let Ok(date) = NaiveDate::parse_from_str(text, fmt) {
        let naive = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("datetime.parse() could not construct midnight for '{}'", text))?;
        let dt = Local
            .from_local_datetime(&naive)
            .single()
            .ok_or_else(|| format!("datetime.parse() produced an ambiguous local date for '{}'", text))?;
        return Ok(dt.timestamp() as f64);
    }

    Err(format!(
        "datetime.parse() could not parse '{}' with format '{}'",
        text, fmt
    ))
}

pub fn parts(timestamp: f64) -> Result<DateTimeParts, String> {
    let dt = local_from_timestamp(timestamp)?;
    Ok(DateTimeParts {
        year: dt.year() as i64,
        month: dt.month() as i64,
        day: dt.day() as i64,
        hour: dt.hour() as i64,
        minute: dt.minute() as i64,
        second: dt.second() as i64,
        weekday: dt.weekday().num_days_from_sunday() as i64,
        yearday: dt.ordinal() as i64,
    })
}

pub fn add_seconds(timestamp: f64, seconds: f64) -> Result<f64, String> {
    validate_finite(timestamp, "datetime.add_seconds() timestamp")?;
    validate_finite(seconds, "datetime.add_seconds() seconds")?;
    let out = timestamp + seconds;
    validate_finite(out, "datetime.add_seconds() result")?;
    Ok(out)
}

pub fn diff_seconds(left: f64, right: f64) -> Result<f64, String> {
    validate_finite(left, "datetime.diff_seconds() left timestamp")?;
    validate_finite(right, "datetime.diff_seconds() right timestamp")?;
    let out = left - right;
    validate_finite(out, "datetime.diff_seconds() result")?;
    Ok(out)
}

fn local_from_timestamp(timestamp: f64) -> Result<DateTime<Local>, String> {
    validate_finite(timestamp, "datetime timestamp")?;
    let (secs, nanos) = split_timestamp(timestamp)?;
    Local
        .timestamp_opt(secs, nanos)
        .single()
        .ok_or_else(|| "datetime timestamp out of range".to_string())
}

fn split_timestamp(timestamp: f64) -> Result<(i64, u32), String> {
    if timestamp < i64::MIN as f64 || timestamp > i64::MAX as f64 {
        return Err("datetime timestamp out of range".to_string());
    }

    let mut secs = timestamp.floor() as i64;
    let mut nanos = ((timestamp - secs as f64) * 1_000_000_000.0).round() as i64;
    if nanos >= 1_000_000_000 {
        secs += 1;
        nanos -= 1_000_000_000;
    }
    Ok((secs, nanos as u32))
}

fn normalize_format(fmt: Option<&str>) -> Result<&str, String> {
    match fmt {
        None => Ok(DEFAULT_FORMAT),
        Some(raw) if raw.is_empty() => Err("datetime format must not be empty".to_string()),
        Some(raw) => Ok(raw),
    }
}

fn validate_finite(value: f64, context: &str) -> Result<(), String> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(format!("{context} must be a finite number"))
    }
}

#[cfg(test)]
mod tests {
    use super::{add_seconds, diff_seconds, format, parse, parts, DEFAULT_FORMAT};

    #[test]
    fn round_trips_default_format() {
        let ts = parse("2024-01-02 03:04:05", None).unwrap();
        assert_eq!(format(ts, None).unwrap(), "2024-01-02 03:04:05");
        let fields = parts(ts).unwrap();
        assert_eq!(fields.year, 2024);
        assert_eq!(fields.month, 1);
        assert_eq!(fields.day, 2);
        assert_eq!(fields.hour, 3);
        assert_eq!(fields.minute, 4);
        assert_eq!(fields.second, 5);
        assert_eq!(DEFAULT_FORMAT, "%Y-%m-%d %H:%M:%S");
    }

    #[test]
    fn supports_custom_formats_and_date_only_inputs() {
        let ts = parse("2024/05/06", Some("%Y/%m/%d")).unwrap();
        assert_eq!(format(ts, Some("%Y/%m/%d")).unwrap(), "2024/05/06");
    }

    #[test]
    fn adds_and_diffs_seconds() {
        let base = parse("2024-01-02 03:04:05", None).unwrap();
        let shifted = add_seconds(base, 90.0).unwrap();
        assert_eq!(format(shifted, None).unwrap(), "2024-01-02 03:05:35");
        assert_eq!(diff_seconds(shifted, base).unwrap(), 90.0);
    }
}
