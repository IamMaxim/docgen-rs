//! Date rendering: the default cell format plus a Moment.js format-string subset
//! for `date.format("YYYY-MM-DD")`. Obsidian dates use Moment.js tokens; we
//! implement the common subset and pass through any literal text (incl.
//! `[bracketed]` literals).

use crate::value::BaseDate;

const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const DAYS_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Day-of-week 0=Sun..6=Sat, from the epoch-day count (epoch 1970-01-01 was a
/// Thursday = 4).
fn weekday(d: &BaseDate) -> usize {
    let days = d.epoch_millis().div_euclid(86_400_000);
    (((days % 7) + 4 + 7) % 7) as usize
}

/// The default rendering used for a table cell / string coercion: `YYYY-MM-DD`
/// for a bare date, `YYYY-MM-DD HH:mm:ss` when the source carried a time.
pub fn default_date(d: &BaseDate) -> String {
    if d.has_time {
        format(d, "YYYY-MM-DD HH:mm:ss")
    } else {
        format(d, "YYYY-MM-DD")
    }
}

/// Render `date` using a Moment.js-style format string. Supported tokens:
/// `YYYY YY MMMM MMM MM M DD D dddd ddd HH H hh h mm m ss s SSS A a Do`.
/// `[literal]` brackets emit their contents verbatim; any other character is a
/// literal.
pub fn format(d: &BaseDate, fmt: &str) -> String {
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // Bracketed literal: [text] → text.
        if c == '[' {
            i += 1;
            while i < chars.len() && chars[i] != ']' {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip ']'
            }
            continue;
        }
        // `Do` (ordinal day) is a two-char token, not a same-char run.
        if c == 'D' && chars.get(i + 1) == Some(&'o') {
            out.push_str(&ordinal(d.day));
            i += 2;
            continue;
        }
        // A run of the same token char.
        if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i] == c {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            out.push_str(&token(d, &run));
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn hour12(h: u32) -> u32 {
    let h = h % 12;
    if h == 0 {
        12
    } else {
        h
    }
}

fn ordinal(n: u32) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

fn token(d: &BaseDate, run: &str) -> String {
    match run {
        "YYYY" => format!("{:04}", d.year),
        "YY" => format!("{:02}", (d.year % 100).abs()),
        "MMMM" => MONTHS_LONG[(d.month.clamp(1, 12) - 1) as usize].to_string(),
        "MMM" => MONTHS_SHORT[(d.month.clamp(1, 12) - 1) as usize].to_string(),
        "MM" => format!("{:02}", d.month),
        "M" => d.month.to_string(),
        "DD" => format!("{:02}", d.day),
        "D" => d.day.to_string(),
        "Do" => ordinal(d.day),
        "dddd" => DAYS_LONG[weekday(d)].to_string(),
        "ddd" => DAYS_SHORT[weekday(d)].to_string(),
        "dd" => DAYS_SHORT[weekday(d)][..2].to_string(),
        "HH" => format!("{:02}", d.hour),
        "H" => d.hour.to_string(),
        "hh" => format!("{:02}", hour12(d.hour)),
        "h" => hour12(d.hour).to_string(),
        "mm" => format!("{:02}", d.minute),
        "m" => d.minute.to_string(),
        "ss" => format!("{:02}", d.second),
        "s" => d.second.to_string(),
        "SSS" => format!("{:03}", d.millisecond),
        "A" => if d.hour < 12 { "AM" } else { "PM" }.to_string(),
        "a" => if d.hour < 12 { "am" } else { "pm" }.to_string(),
        // Unknown token run: emit verbatim (Moment would too for unknowns).
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i64, mo: u32, dd: u32, h: u32, mi: u32, s: u32) -> BaseDate {
        BaseDate {
            year: y,
            month: mo,
            day: dd,
            hour: h,
            minute: mi,
            second: s,
            millisecond: 0,
            has_time: h != 0 || mi != 0 || s != 0,
        }
    }

    #[test]
    fn iso_date() {
        let d = date(2024, 1, 2, 0, 0, 0);
        assert_eq!(format(&d, "YYYY-MM-DD"), "2024-01-02");
    }

    #[test]
    fn date_time() {
        let d = date(2024, 12, 5, 9, 8, 7);
        assert_eq!(format(&d, "YYYY-MM-DD HH:mm:ss"), "2024-12-05 09:08:07");
    }

    #[test]
    fn month_names_and_ordinal() {
        let d = date(2024, 3, 1, 0, 0, 0);
        assert_eq!(format(&d, "MMMM Do, YYYY"), "March 1st, 2024");
        assert_eq!(format(&d, "MMM"), "Mar");
    }

    #[test]
    fn twelve_hour_and_meridiem() {
        let d = date(2024, 1, 1, 13, 5, 0);
        assert_eq!(format(&d, "h:mm A"), "1:05 PM");
        let m = date(2024, 1, 1, 0, 0, 0);
        assert_eq!(format(&m, "h:mm a"), "12:00 am");
    }

    #[test]
    fn bracketed_literal() {
        let d = date(2024, 1, 2, 0, 0, 0);
        assert_eq!(format(&d, "[Year] YYYY"), "Year 2024");
    }

    #[test]
    fn weekday_token() {
        // 2024-01-01 was a Monday.
        let d = date(2024, 1, 1, 0, 0, 0);
        assert_eq!(format(&d, "dddd"), "Monday");
        assert_eq!(format(&d, "ddd"), "Mon");
    }

    #[test]
    fn default_bare_vs_time() {
        assert_eq!(default_date(&date(2024, 1, 2, 0, 0, 0)), "2024-01-02");
        assert_eq!(
            default_date(&date(2024, 1, 2, 3, 4, 5)),
            "2024-01-02 03:04:05"
        );
    }
}
