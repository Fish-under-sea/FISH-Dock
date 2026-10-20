// Numbers as the example scripts wrote them (examples/widgets/*.ps1), so the
// widgets she makes herself read exactly as they did when PowerShell made
// them. .NET rounds in more than one way, and the scripts met each:
//
//   [Math]::Round(x), [int]x      to the even one at a half: 12.5 is 12,
//                                 13.5 is 14 (the weather's degrees, the
//                                 pomodoro's seconds left)
//   [Math]::Round(x, 4)           the same at four places, by scaling (the
//                                 stock's curve)
//   '{0:N2}' -f x                 the exact value, to the even one at a half
//                                 (0.125 is 0.12), with a comma every three
//                                 places (the stock's price and change):
//                                 what Rust's own {:.2} does, commas added
//   '{0:0.#}' -f x                fifteen significant digits first, then a
//                                 half up (48.15 is 48.2, 1.25 is 1.3; the
//                                 stock's volume, 3.9M)
//   "$x"                          a double as it reads back (2.5, 25), as
//                                 Rust writes one
//
// The scripts formatted with the culture of the machine; Chinese and English
// Windows write 1,329.00 alike, which is what these write everywhere.

// [Math]::Round(x) and PowerShell's [int]: a half to the even one.
pub fn round_even(x: f64) -> f64 {
    x.round_ties_even()
}

// [Math]::Round(x, digits), as .NET does it: scaled, rounded to the even
// one, scaled back (the same steps, so the same doubles).
pub fn round_to(x: f64, digits: i32) -> f64 {
    if !x.is_finite() || x.abs() >= 1e16 {
        return x;
    }
    let p = 10f64.powi(digits);
    (x * p).round_ties_even() / p
}

// A whole number of degrees: "$([Math]::Round([double]$t))°". A temperature
// just under nought rounds to -0 there, and .NET writes it so: "-0°", kept.
pub fn deg(t: f64) -> String {
    let r = round_even(t);
    if r == 0.0 && r.is_sign_negative() {
        return "-0°".into();
    }
    format!("{}°", r as i64)
}

// '{0:N<places>}' -f x: the exact value rounded (a half to the even one),
// a comma between each three whole places.
pub fn grouped(x: f64, places: usize) -> String {
    let text = format!("{:.*}", places, x);
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", text.as_str()),
    };
    let (whole, part) = match digits.split_once('.') {
        Some((w, p)) => (w, Some(p)),
        None => (digits, None),
    };
    let mut out = String::from(sign);
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if let Some(part) = part {
        out.push('.');
        out.push_str(part);
    }
    out
}

// '{0:0.#}' (one place at most, none when it is nought) or '{0:0}' (none):
// .NET takes the double to fifteen significant digits, then rounds those a
// half up. For a number nought or more.
pub fn custom(x: f64, places: usize) -> String {
    if !x.is_finite() || x <= 0.0 {
        return "0".into();
    }
    // d.dddddddddddddde<exp>: the fifteen digits, and where the point goes.
    let sci = format!("{:.14e}", x);
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let mut digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    let mut point = exp.parse::<i32>().unwrap_or(0) + 1;
    // Rounded a half up at the last place kept.
    let cut = point + places as i32;
    if cut < 0 {
        return "0".into();
    }
    let cut = cut as usize;
    if cut < digits.len() {
        let up = digits[cut] >= 5;
        digits.truncate(cut);
        if up {
            let mut i = digits.len();
            loop {
                if i == 0 {
                    digits.insert(0, 1);
                    point += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    }
    let at = |i: i32| if i >= 0 { digits.get(i as usize).copied().unwrap_or(0) } else { 0 };
    let whole: String = if point <= 0 { "0".into() } else { (0..point).map(|i| char::from(b'0' + at(i))).collect() };
    let part: String = (point..point + places as i32).map(|i| char::from(b'0' + at(i))).collect();
    let part = part.trim_end_matches('0');
    if part.is_empty() {
        whole
    } else {
        format!("{whole}.{part}")
    }
}

// "$x" for a double the person typed (minutes): as it reads back.
pub fn plain(x: f64) -> String {
    format!("{x}")
}

// PowerShell's [int] of a number typed as text ("2.5" is 2, "3.5" is 4):
// read as a double, a half to the even one; 0 for none.
pub fn int_of(text: &str) -> Option<i64> {
    let n: f64 = text.trim().parse().ok()?;
    n.is_finite().then(|| round_even(n).clamp(i32::MIN as f64, i32::MAX as f64) as i64)
}

// [uri]::EscapeDataString: every byte but the unreserved ones (letters,
// digits, - . _ ~) as %XX.
pub fn escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// A widget id from a name the person gave (a symbol, a repository): lower
// case, anything but a-z 0-9 _ - made a dash ($s.ToLower() -replace
// '[^a-z0-9_-]', '-').
pub fn id_part(s: &str) -> String {
    s.to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' { c } else { '-' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each as PowerShell 7 wrote it (zh-CN, 2026-10-18).
    #[test]
    fn numbers_read_as_powershell_wrote_them() {
        assert_eq!((round_even(2.5), round_even(12.5), round_even(13.5)), (2.0, 12.0, 14.0));
        assert_eq!((deg(25.7), deg(12.5), deg(-0.4), deg(-0.5), deg(-0.6), deg(0.0)), ("26°".into(), "12°".into(), "-0°".into(), "-0°".into(), "-1°".into(), "0°".into()));
        assert_eq!(grouped(0.125, 2), "0.12");
        assert_eq!(grouped(2.5, 0), "2");
        assert_eq!(grouped(3.5, 0), "4");
        assert_eq!(grouped(1234.5, 0), "1,234");
        assert_eq!(grouped(1.005, 2), "1.00");
        assert_eq!(grouped(7.12345, 4), "7.1235");
        assert_eq!(grouped(1500.125, 2), "1,500.12");
        assert_eq!(grouped(1234567.891, 2), "1,234,567.89");
        assert_eq!(grouped(81326.906, 0), "81,327");
        assert_eq!(grouped(-0.001, 2), "-0.00");
        assert_eq!(grouped(999.999, 2), "1,000.00");
        assert_eq!(custom(48.15, 1), "48.2");
        assert_eq!(custom(1.25, 1), "1.3");
        assert_eq!(custom(1.35, 1), "1.4");
        assert_eq!(custom(812.5, 0), "813");
        assert_eq!(custom(813.5, 0), "814");
        assert_eq!(custom(3.850175, 1), "3.9");
        assert_eq!(custom(59.9, 1), "59.9");
        assert_eq!(custom(9.96, 1), "10");
        assert_eq!(custom(999.97, 1), "1000");
        assert_eq!(custom(0.04, 1), "0");
        assert_eq!(custom(0.05, 1), "0.1");
        assert_eq!(custom(12.0, 1), "12");
        assert_eq!(round_to(1316.43994140625, 4), 1316.4399);
        assert_eq!(round_to(25.66, 1), 25.7);
        assert_eq!((plain(25.0), plain(2.5), plain(0.25)), ("25".into(), "2.5".into(), "0.25".into()));
        assert_eq!((int_of("50"), int_of("2.5"), int_of("3.5"), int_of(" 7 "), int_of("x")), (Some(50), Some(2), Some(4), Some(7), None));
    }

    #[test]
    fn names_go_into_addresses_and_ids_as_the_scripts_put_them() {
        assert_eq!(escape("USDCNY=X"), "USDCNY%3DX");
        assert_eq!(escape("^GSPC"), "%5EGSPC");
        assert_eq!(escape("上海"), "%E4%B8%8A%E6%B5%B7");
        assert_eq!(escape("New York"), "New%20York");
        assert_eq!(escape("a-b.c_d~e"), "a-b.c_d~e");
        assert_eq!(id_part("600519.SS"), "600519-ss");
        assert_eq!(id_part("USDCNY=X"), "usdcny-x");
        assert_eq!(id_part("Octo.Cat/Hello_World"), "octo-cat-hello_world");
    }
}
