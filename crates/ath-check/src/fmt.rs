//! Formatação numérica em português (vírgula decimal, ponto de milhar).

pub fn int(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

pub fn dec(x: f64, places: usize) -> String {
    let s = format!("{:.*}", places, x);
    let (int_part, frac) = match s.split_once('.') {
        Some((a, b)) => (a.to_string(), Some(b.to_string())),
        None => (s.clone(), None),
    };
    let neg = int_part.starts_with('-');
    let digits = int_part.trim_start_matches('-');
    let grouped = int(digits.parse().unwrap_or(0));
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&grouped);
    if let Some(f) = frac {
        out.push(',');
        out.push_str(&f);
    }
    out
}

pub fn secs(x: f64) -> String {
    format!("{} s", dec(x, 1))
}

pub fn usd(x: f64) -> String {
    let places = if x != 0.0 && x.abs() < 0.1 { 4 } else { 2 };
    format!("US$ {}", dec(x, places))
}

pub fn tok(n: u64) -> String {
    format!("{} tok", int(n))
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats() {
        assert_eq!(super::int(29200), "29.200");
        assert_eq!(super::dec(86.66, 1), "86,7");
        assert_eq!(super::usd(0.102), "US$ 0,10");
        assert_eq!(super::usd(0.0327), "US$ 0,0327");
    }
}
