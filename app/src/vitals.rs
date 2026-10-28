//! The machine Cove runs on, as the ndexr bar shows admins: CPU busy since
//! the last ask (/proc/stat), memory in use (/proc/meminfo, total less
//! available) and the root filesystem's free space (statvfs). Read by
//! `cove multiuser` for an ndexr admin (Query::HostVitals).

use crate::api::Vitals;
use std::sync::Mutex;

/// The last /proc/stat sample: (busy, total) jiffies, across every core.
static LAST: Mutex<Option<(u64, u64)>> = Mutex::new(None);

fn cpu_sample() -> Option<(u64, u64, u32)> {
    let text = std::fs::read_to_string("/proc/stat").ok()?;
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let v: Vec<u64> = line.split_whitespace().skip(1).filter_map(|x| x.parse().ok()).collect();
    if v.len() < 5 {
        return None;
    }
    let total: u64 = v.iter().sum();
    // idle + iowait are not busy.
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    let cores = text.lines().filter(|l| l.starts_with("cpu") && l.as_bytes().get(3).is_some_and(u8::is_ascii_digit)).count() as u32;
    Some((total - idle, total, cores))
}

/// Busy share between two samples, 0-100.
fn busy(prev: (u64, u64), now: (u64, u64)) -> f64 {
    let (db, dt) = (now.0.saturating_sub(prev.0), now.1.saturating_sub(prev.1));
    if dt == 0 { 0.0 } else { (100.0 * db as f64 / dt as f64).clamp(0.0, 100.0) }
}

/// Memory in use from /proc/meminfo's text, 0-100.
fn mem_pct(meminfo: &str) -> Option<f64> {
    let kb = |key: &str| meminfo.lines().find_map(|l| l.strip_prefix(key)).and_then(|r| r.split_whitespace().next()?.parse::<f64>().ok());
    let (total, avail) = (kb("MemTotal:")?, kb("MemAvailable:")?);
    (total > 0.0).then(|| (100.0 * (total - avail) / total).clamp(0.0, 100.0))
}

fn disk(path: &str) -> Option<(u64, u64)> {
    let c = std::ffi::CString::new(path).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: a valid C string and a zeroed statvfs for the call to fill.
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let f = s.f_frsize as u64;
    Some((s.f_bavail as u64 * f, s.f_blocks as u64 * f))
}

pub fn read() -> Result<Vitals, String> {
    let (b, t, cores) = cpu_sample().ok_or("no /proc/stat")?;
    let cpu_pct = {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let prev = last.replace((b, t));
        match prev {
            Some(p) => busy(p, (b, t)),
            // The first ask has nothing to compare with: the load average
            // over a minute, as a share of the cores.
            None => std::fs::read_to_string("/proc/loadavg").ok().and_then(|l| l.split_whitespace().next()?.parse::<f64>().ok()).map_or(0.0, |l| (100.0 * l / cores.max(1) as f64).clamp(0.0, 100.0)),
        }
    };
    let mem_pct = std::fs::read_to_string("/proc/meminfo").ok().as_deref().and_then(mem_pct).unwrap_or(0.0);
    let (disk_free, disk_total) = disk("/").unwrap_or((0, 0));
    Ok(Vitals { cpu_pct, mem_pct, disk_free, disk_total, cores })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_share_and_memory_read_as_the_bar_shows_them() {
        assert_eq!(busy((100, 1000), (150, 1100)), 50.0);
        assert_eq!(busy((100, 1000), (100, 1000)), 0.0);
        let m = "MemTotal:       64000000 kB\nMemFree:  1000 kB\nMemAvailable:   16000000 kB\n";
        assert_eq!(mem_pct(m), Some(75.0));
        let v = read().unwrap();
        assert!(v.disk_total > 0 && v.disk_free <= v.disk_total && v.cores > 0);
    }
}
