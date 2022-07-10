use std::cmp;
use std::fmt::{self, Display, Formatter, Pointer};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::LogError as Error;

/// HTTP timestamp type.
///
/// Parse using `FromStr` impl.
/// Format using the `Display` trait.
/// Convert timestamp into/from `SytemTime` to use.
/// Supports comparsion and sorting.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct LogDate {
    /// 0...999999999
    pub nano: u32,
    /// 0...59
    pub sec: u8,
    /// 0...59
    pub min: u8,
    /// 0...23
    pub hour: u8,
    /// 1...31
    pub day: u8,
    /// 1...12
    pub mon: u8,
    /// 1970...9999
    pub year: u16,
}

impl LogDate {
    fn is_valid(&self) -> bool {
        self.sec < 60
            && self.min < 60
            && self.hour < 24
            && self.day > 0
            && self.day < 32
            && self.mon > 0
            && self.mon <= 12
            && self.year >= 1970
            && self.year <= 9999
            && &LogDate::from(SystemTime::from(*self)) == self
    }
}

impl From<SystemTime> for LogDate {
    fn from(v: SystemTime) -> LogDate {
        let dur = v
            .duration_since(UNIX_EPOCH)
            .expect("all times should be after the epoch");
        let secs_since_epoch = dur.as_secs();

        if secs_since_epoch >= 253402300800 {
            // year 9999
            panic!("date must be before year 9999");
        }

        /* 2000-03-01 (mod 400 year, immediately after feb29 */
        const LEAPOCH: i64 = 11017;
        const DAYS_PER_400Y: i64 = 365 * 400 + 97;
        const DAYS_PER_100Y: i64 = 365 * 100 + 24;
        const DAYS_PER_4Y: i64 = 365 * 4 + 1;

        let days = (secs_since_epoch / 86400) as i64 - LEAPOCH;
        let secs_of_day = secs_since_epoch % 86400;

        let mut qc_cycles = days / DAYS_PER_400Y;
        let mut remdays = days % DAYS_PER_400Y;

        if remdays < 0 {
            remdays += DAYS_PER_400Y;
            qc_cycles -= 1;
        }

        let mut c_cycles = remdays / DAYS_PER_100Y;
        if c_cycles == 4 {
            c_cycles -= 1;
        }
        remdays -= c_cycles * DAYS_PER_100Y;

        let mut q_cycles = remdays / DAYS_PER_4Y;
        if q_cycles == 25 {
            q_cycles -= 1;
        }
        remdays -= q_cycles * DAYS_PER_4Y;

        let mut remyears = remdays / 365;
        if remyears == 4 {
            remyears -= 1;
        }
        remdays -= remyears * 365;

        let mut year = 2000 + remyears + 4 * q_cycles + 100 * c_cycles + 400 * qc_cycles;

        let months = [31, 30, 31, 30, 31, 31, 30, 31, 30, 31, 31, 29];
        let mut mon = 0;
        for mon_len in months.iter() {
            mon += 1;
            if remdays < *mon_len {
                break;
            }
            remdays -= *mon_len;
        }
        let mday = remdays + 1;
        let mon = if mon + 2 > 12 {
            year += 1;
            mon - 10
        } else {
            mon + 2
        };

        LogDate {
            nano: (dur - Duration::from_secs(dur.as_secs())).as_nanos() as u32,
            sec: (secs_of_day % 60) as u8,
            min: ((secs_of_day % 3600) / 60) as u8,
            hour: (secs_of_day / 3600) as u8,
            day: mday as u8,
            mon: mon as u8,
            year: year as u16,
        }
    }
}

impl From<LogDate> for SystemTime {
    fn from(v: LogDate) -> SystemTime {
        let leap_years =
            ((v.year - 1) - 1968) / 4 - ((v.year - 1) - 1900) / 100 + ((v.year - 1) - 1600) / 400;
        let mut ydays = match v.mon {
            1 => 0,
            2 => 31,
            3 => 59,
            4 => 90,
            5 => 120,
            6 => 151,
            7 => 181,
            8 => 212,
            9 => 243,
            10 => 273,
            11 => 304,
            12 => 334,
            _ => unreachable!(),
        } + v.day as u64
            - 1;
        if is_leap_year(v.year) && v.mon > 2 {
            ydays += 1;
        }
        let days = (v.year as u64 - 1970) * 365 + leap_years as u64 + ydays;
        UNIX_EPOCH
            + Duration::from_secs(
            v.sec as u64 + v.min as u64 * 60 + v.hour as u64 * 3600 + days * 86400,
        )
    }
}

impl FromStr for LogDate {
    type Err = Error;

    fn from_str(s: &str) -> Result<LogDate, Error> {
        //"0000-00-00 00:00:00.000000000";
        let mut date = LogDate {
            nano: 0,
            sec: 0,
            min: 0,
            hour: 0,
            day: 0,
            mon: 0,
            year: 0,
        };
        let bytes = s.as_bytes();
        if bytes.len() > 4 {
            if let Ok(year) = std::str::from_utf8(&bytes[0..4]).unwrap_or_default().parse::<u16>() {
                date.year = year;
            }
            if let Ok(mon) = std::str::from_utf8(&bytes[5..7]).unwrap_or_default().parse::<u8>() {
                date.mon = mon;
            }
            if let Ok(day) = std::str::from_utf8(&bytes[8..10]).unwrap_or_default().parse::<u8>() {
                date.day = day;
            }
            if let Ok(hour) = std::str::from_utf8(&bytes[11..13]).unwrap_or_default().parse::<u8>() {
                date.hour = hour;
            }
            if let Ok(min) = std::str::from_utf8(&bytes[14..16]).unwrap_or_default().parse::<u8>() {
                date.min = min;
            }
            if let Ok(sec) = std::str::from_utf8(&bytes[17..19]).unwrap_or_default().parse::<u8>() {
                date.sec = sec;
            }
            if let Ok(ns) = std::str::from_utf8(&bytes[20..29]).unwrap_or_default().parse::<u32>() {
                date.nano = ns;
            }
        }
        Ok(date)
    }
}


impl Display for LogDate {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let mut buf: [u8; 20] = *b"0000-00-00 00:00:00.";

        buf[0] = b'0' + (self.year / 1000) as u8;
        buf[1] = b'0' + (self.year / 100 % 10) as u8;
        buf[2] = b'0' + (self.year / 10 % 10) as u8;
        buf[3] = b'0' + (self.year % 10) as u8;


        buf[5] = b'0' + (self.mon / 10) as u8;
        buf[6] = b'0' + (self.mon % 10) as u8;

        buf[8] = b'0' + (self.day / 10) as u8;
        buf[9] = b'0' + (self.day % 10) as u8;

        buf[11] = b'0' + (self.hour / 10) as u8;
        buf[12] = b'0' + (self.hour % 10) as u8;
        buf[14] = b'0' + (self.min / 10) as u8;
        buf[15] = b'0' + (self.min % 10) as u8;
        buf[17] = b'0' + (self.sec / 10) as u8;
        buf[18] = b'0' + (self.sec % 10) as u8;

        buf[19] = b'.';

        f.write_str(std::str::from_utf8(&buf[..]).unwrap())?;
        write!(f, "{:09}", self.nano)
    }
}

impl Ord for LogDate {
    fn cmp(&self, other: &LogDate) -> cmp::Ordering {
        SystemTime::from(*self).cmp(&SystemTime::from(*other))
    }
}

impl PartialOrd for LogDate {
    fn partial_cmp(&self, other: &LogDate) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn toint_1(x: u8) -> Result<u8, Error> {
    let result = x.wrapping_sub(b'0');
    if result < 10 {
        Ok(result)
    } else {
        Err(Error::default())
    }
}

fn toint_2(s: &[u8]) -> Result<u8, Error> {
    let high = s[0].wrapping_sub(b'0');
    let low = s[1].wrapping_sub(b'0');

    if high < 10 && low < 10 {
        Ok(high * 10 + low)
    } else {
        Err(Error::default())
    }
}

#[allow(clippy::many_single_char_names)]
fn toint_4(s: &[u8]) -> Result<u16, Error> {
    let a = u16::from(s[0].wrapping_sub(b'0'));
    let b = u16::from(s[1].wrapping_sub(b'0'));
    let c = u16::from(s[2].wrapping_sub(b'0'));
    let d = u16::from(s[3].wrapping_sub(b'0'));

    if a < 10 && b < 10 && c < 10 && d < 10 {
        Ok(a * 1000 + b * 100 + c * 10 + d)
    } else {
        Err(Error::default())
    }
}

fn parse_imf_fixdate(s: &[u8]) -> Result<LogDate, Error> {
    // Example: `Sun, 06 Nov 1994 08:49:37 GMT`
    if s.len() != 29 || &s[25..] != b" GMT" || s[16] != b' ' || s[19] != b':' || s[22] != b':' {
        return Err(Error::default());
    }
    Ok(LogDate {
        nano: 0,
        sec: toint_2(&s[23..25])?,
        min: toint_2(&s[20..22])?,
        hour: toint_2(&s[17..19])?,
        day: toint_2(&s[5..7])?,
        mon: match &s[7..12] {
            b" Jan " => 1,
            b" Feb " => 2,
            b" Mar " => 3,
            b" Apr " => 4,
            b" May " => 5,
            b" Jun " => 6,
            b" Jul " => 7,
            b" Aug " => 8,
            b" Sep " => 9,
            b" Oct " => 10,
            b" Nov " => 11,
            b" Dec " => 12,
            _ => return Err(Error::default()),
        },
        year: toint_4(&s[12..16])?,
    })
}

fn is_leap_year(y: u16) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

#[cfg(test)]
mod test {
    use std::str::FromStr;
    use crate::date::LogDate;

    #[test]
    fn test_date() {
        let d = LogDate::from_str("1234-12-13 11:12:13.112345678").unwrap();
        println!("{}", d);
        assert_eq!("1234-12-13 11:12:13.112345678".to_string(),d.to_string());
    }
}


