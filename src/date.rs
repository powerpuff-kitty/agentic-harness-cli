use std::cell::RefCell;
thread_local! {static AS_OF:RefCell<Option<String>>=const {RefCell::new(None)};}
fn leap(y: u32) -> bool {
    y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400))
}
pub fn validate(value: &str) -> Result<(), String> {
    if value.len() != 10
        || value.as_bytes()[4] != b'-'
        || value.as_bytes()[7] != b'-'
        || !value
            .chars()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return Err("date must use YYYY-MM-DD".into());
    }
    let y = value[..4].parse::<u32>().unwrap();
    let m = value[5..7].parse::<usize>().unwrap();
    let d = value[8..].parse::<u32>().unwrap();
    let months = [
        31,
        if leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if y == 0 || !(1..=12).contains(&m) || d == 0 || d > months[m - 1] {
        return Err("invalid calendar date".into());
    }
    Ok(())
}
pub fn set(value: &str) -> Result<(), String> {
    validate(value)?;
    AS_OF.with(|date| *date.borrow_mut() = Some(value.into()));
    Ok(())
}
pub fn today() -> String {
    if let Some(value) = AS_OF.with(|d| d.borrow().clone()) {
        return value;
    }
    let mut days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400;
    let mut y = 1970;
    loop {
        let count = if leap(y) { 366 } else { 365 };
        if days < count {
            break;
        }
        days -= count;
        y += 1;
    }
    let months = [
        31,
        if leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1;
    for count in months {
        if days < count {
            break;
        }
        days -= count;
        m += 1;
    }
    format!("{y:04}-{m:02}-{:02}", days + 1)
}
