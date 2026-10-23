use anyhow::{Result, bail};
use time::{OffsetDateTime, UtcOffset, Weekday};

pub const IST_OFFSET_SECONDS: i64 = 19_800;
const DAY_SECONDS: i64 = 86_400;
const WEEKDAYS: [Weekday; 7] = [
    Weekday::Monday,
    Weekday::Tuesday,
    Weekday::Wednesday,
    Weekday::Thursday,
    Weekday::Friday,
    Weekday::Saturday,
    Weekday::Sunday,
];

pub fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

pub fn ist_day_number(unix: i64) -> i64 {
    (unix + IST_OFFSET_SECONDS).div_euclid(DAY_SECONDS)
}

fn ist_minute_of_day(unix: i64) -> i64 {
    (unix + IST_OFFSET_SECONDS).rem_euclid(DAY_SECONDS) / 60
}

fn ist_weekday(unix: i64) -> Weekday {
    let offset =
        UtcOffset::from_whole_seconds(IST_OFFSET_SECONDS as i32).expect("IST is a valid offset");
    OffsetDateTime::from_unix_timestamp(unix)
        .map(|moment| moment.to_offset(offset).weekday())
        .unwrap_or(Weekday::Monday)
}

pub fn ist_iso(unix: i64) -> String {
    let offset =
        UtcOffset::from_whole_seconds(IST_OFFSET_SECONDS as i32).expect("IST is a valid offset");
    OffsetDateTime::from_unix_timestamp(unix)
        .ok()
        .and_then(|moment| {
            moment
                .to_offset(offset)
                .format(&time::format_description::well_known::Rfc3339)
                .ok()
        })
        .unwrap_or_default()
}

pub fn describe(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let (hours, minutes, rest) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    match (hours, minutes) {
        (0, 0) => format!("{rest}s"),
        (0, _) => format!("{minutes}m {rest}s"),
        _ => format!("{hours}h {minutes}m"),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginWindow {
    start_minute: i64,
    end_minute: i64,
    days: [bool; 7],
}

impl LoginWindow {
    pub fn parse(window: &str, days: &str) -> Result<Self> {
        let Some((start, end)) = window.trim().split_once('-') else {
            bail!("CBL_LOGIN_WINDOW must look like 08:00-08:30, got {window:?}");
        };
        let start_minute = parse_clock(start)?;
        let end_minute = parse_clock(end)?;
        if end_minute <= start_minute {
            bail!("CBL_LOGIN_WINDOW must end after it starts, got {window:?}");
        }
        Ok(Self {
            start_minute,
            end_minute,
            days: parse_days(days)?,
        })
    }

    pub fn label(&self) -> String {
        format!(
            "{}-{} IST",
            clock_label(self.start_minute),
            clock_label(self.end_minute)
        )
    }

    pub fn is_login_day(&self, unix: i64) -> bool {
        let weekday = ist_weekday(unix);
        WEEKDAYS
            .iter()
            .position(|day| *day == weekday)
            .is_some_and(|index| self.days[index])
    }

    pub fn is_open(&self, unix: i64) -> bool {
        let minute = ist_minute_of_day(unix);
        self.is_login_day(unix) && minute >= self.start_minute && minute < self.end_minute
    }

    pub fn has_passed_today(&self, unix: i64) -> bool {
        self.is_login_day(unix) && ist_minute_of_day(unix) >= self.end_minute
    }

    pub fn seconds_left(&self, unix: i64) -> i64 {
        if !self.is_open(unix) {
            return 0;
        }
        let into_day = (unix + IST_OFFSET_SECONDS).rem_euclid(DAY_SECONDS);
        self.end_minute * 60 - into_day
    }

    pub fn seconds_until_open(&self, unix: i64) -> i64 {
        if self.is_open(unix) {
            return 0;
        }
        let into_day = (unix + IST_OFFSET_SECONDS).rem_euclid(DAY_SECONDS);
        let start_today = unix - into_day + self.start_minute * 60;
        (0..=7)
            .map(|days_ahead| start_today + days_ahead * DAY_SECONDS)
            .find(|start| *start > unix && self.is_login_day(*start))
            .map_or(DAY_SECONDS, |start| start - unix)
    }
}

fn parse_clock(value: &str) -> Result<i64> {
    let Some((hours, minutes)) = value.trim().split_once(':') else {
        bail!("a time must look like HH:MM, got {value:?}");
    };
    let (Ok(hours), Ok(minutes)) = (hours.parse::<i64>(), minutes.parse::<i64>()) else {
        bail!("a time must look like HH:MM, got {value:?}");
    };
    if !(0..24).contains(&hours) || !(0..60).contains(&minutes) {
        bail!("{value:?} is not a time of day");
    }
    Ok(hours * 60 + minutes)
}

fn clock_label(minute: i64) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

fn day_index(name: &str) -> Result<usize> {
    const NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let key = name.trim().to_ascii_lowercase();
    NAMES
        .iter()
        .position(|day| key.starts_with(day) && !key.is_empty())
        .ok_or_else(|| anyhow::anyhow!("{name:?} is not a day of the week"))
}

/// Words that mean every day of the week, alone in CBL_LOGIN_DAYS.
const EVERY_DAY: [&str; 4] = ["everyday", "every day", "daily", "all"];

fn parse_days(value: &str) -> Result<[bool; 7]> {
    let normalised = value.trim().to_ascii_lowercase();
    if EVERY_DAY.contains(&normalised.as_str()) {
        return Ok([true; 7]);
    }
    let mut days = [false; 7];
    for part in value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match part.split_once('-') {
            Some((from, to)) => {
                let (from, to) = (day_index(from)?, day_index(to)?);
                if to < from {
                    bail!("a day range must run forwards, got {part:?}");
                }
                days[from..=to].iter_mut().for_each(|day| *day = true);
            }
            None => days[day_index(part)?] = true,
        }
    }
    if !days.contains(&true) {
        bail!("CBL_LOGIN_DAYS names no day");
    }
    Ok(days)
}

#[cfg(test)]
#[path = "../tests/unit/clock.rs"]
mod tests;
