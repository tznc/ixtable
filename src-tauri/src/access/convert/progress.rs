//! The time an import has left, from its progress so far.

/// "about 2 minutes left" from the rows copied so far, or None while too early to tell.
pub fn time_left(done: u64, total: u64, elapsed: std::time::Duration) -> Option<String> {
    if done == 0 || total <= done || elapsed.as_secs_f64() < 2.0 {
        return None;
    }
    let secs = elapsed.as_secs_f64() / done as f64 * (total - done) as f64;
    let plural = |n: u64, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    Some(if secs < 10.0 {
        "a few seconds left".into()
    } else if secs < 60.0 {
        format!("about {} left", plural(((secs / 10.0).round() * 10.0) as u64, "second"))
    } else if secs < 3600.0 {
        format!("about {} left", plural((secs / 60.0).round() as u64, "minute"))
    } else {
        let minutes = (secs / 60.0).round() as u64;
        format!("about {} {} left", plural(minutes / 60, "hour"), plural(minutes % 60, "minute"))
    })
}
