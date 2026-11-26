//! A number as PowerShell prints it. The stick-fit sum that lived here left
//! on 2026-11-26 (the owner): nothing stages files to the stick.

/// A number the way PowerShell's `{0:N2}` prints it: two decimals, halves
/// rounded away from zero, thousands separated by commas.
pub fn n2(x: f64) -> String {
    // .NET Framework rounds from the 15-digit decimal form of the number
    let whole_digits = if x.abs() < 1.0 { 1 } else { x.abs().log10().floor() as usize + 1 };
    let shown = format!("{:.*}", 15usize.saturating_sub(whole_digits).max(2), x.abs());
    let (int_part, frac) = shown.split_once('.').unwrap_or((&shown, ""));
    let mut cents: u128 = format!("{int_part}{}", &frac[..2]).parse().unwrap_or(0);
    if frac.as_bytes().get(2).is_some_and(|d| *d >= b'5') {
        cents += 1;
    }
    let (units, hundredths) = (cents / 100, cents % 100);
    let digits = units.to_string();
    let mut grouped = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{}{grouped}.{hundredths:02}", if x < 0.0 && cents > 0 { "-" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::n2;

    #[test]
    fn n2_prints_as_powershell_does() {
        // answers recorded from Windows PowerShell 5.1 (tests/golden.json)
        assert_eq!(n2(1.125), "1.13");
        assert_eq!(n2(2040.0625), "2,040.06");
        assert_eq!(n2(0.0), "0.00");
        assert_eq!(n2(1500.004999), "1,500.00");
        assert_eq!(n2(1234567.891), "1,234,567.89");
        assert_eq!(n2(0.999), "1.00");
    }
}
