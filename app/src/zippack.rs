//! A file or folder as a .zip, in-process (operator, 2026-11-18: "download
//! options in cove should be tar.gz or zip, both should be available to
//! select prior to download"). No `zip` program needed: a Mac, Windows and a
//! cell all lack one by default. Deflate where it saves space, stored where it
//! does not (images, archives); UTF-8 names; Unix modes kept. No zip64: one
//! download is capped well under 4 GB (host::TRANSFER_MAX).

use std::io::Write;
use std::path::Path;

struct Entry {
    name: String,
    crc: u32,
    csize: u32,
    usize: u32,
    method: u16,
    time: u16,
    date: u16,
    mode: u32,
    offset: u32,
    dir: bool,
}

/// `path` (a file, or a folder and all under it) as a zip whose entries begin
/// with its own name, as tar does. Symbolic links are left out, as the
/// download of a folder would otherwise leave this machine through them.
pub fn zip(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "root".into());
    let mut out: Vec<u8> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    add(path, &name, &mut out, &mut entries, max)?;
    let cd_start = out.len();
    for e in &entries {
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&(0x0300u16 | 20).to_le_bytes()); // made by Unix, 2.0
        out.extend_from_slice(&20u16.to_le_bytes());
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        out.extend_from_slice(&e.method.to_le_bytes());
        out.extend_from_slice(&e.time.to_le_bytes());
        out.extend_from_slice(&e.date.to_le_bytes());
        out.extend_from_slice(&e.crc.to_le_bytes());
        out.extend_from_slice(&e.csize.to_le_bytes());
        out.extend_from_slice(&e.usize.to_le_bytes());
        out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]); // extra, comment, disk, internal attributes
        let kind = if e.dir { 0o040000 } else { 0o100000 };
        out.extend_from_slice(&((kind | e.mode) << 16 | if e.dir { 0x10 } else { 0 }).to_le_bytes());
        out.extend_from_slice(&e.offset.to_le_bytes());
        out.extend_from_slice(e.name.as_bytes());
    }
    let cd_len = out.len() - cd_start;
    if entries.len() > u16::MAX as usize {
        return Err(format!("{} entries - more than a zip without zip64 holds; download it as .tar.gz", entries.len()));
    }
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(cd_len as u32).to_le_bytes());
    out.extend_from_slice(&(cd_start as u32).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}

fn add(path: &Path, name: &str, out: &mut Vec<u8>, entries: &mut Vec<Entry>, max: u64) -> Result<(), String> {
    let m = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if m.file_type().is_symlink() {
        return Ok(());
    }
    let (time, date) = dos_time(m.modified().ok());
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o7777
    };
    #[cfg(not(unix))]
    let mode = if m.is_dir() { 0o755 } else { 0o644 };
    if m.is_dir() {
        push(out, entries, format!("{name}/"), &[], 0, 0, time, date, mode, true, max)?;
        let mut kids: Vec<_> = std::fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?.filter_map(Result::ok).collect();
        kids.sort_by_key(|k| k.file_name());
        for k in kids {
            add(&k.path(), &format!("{name}/{}", k.file_name().to_string_lossy()), out, entries, max)?;
        }
        return Ok(());
    }
    let raw = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let crc = crc32fast::hash(&raw);
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&raw).map_err(|e| e.to_string())?;
    let packed = enc.finish().map_err(|e| e.to_string())?;
    let data = if packed.len() < raw.len() { &packed } else { &raw };
    push(out, entries, name.to_owned(), data, raw.len(), crc, time, date, mode, false, max)
}

#[allow(clippy::too_many_arguments)]
fn push(out: &mut Vec<u8>, entries: &mut Vec<Entry>, name: String, data: &[u8], usize: usize, crc: u32, time: u16, date: u16, mode: u32, dir: bool, max: u64) -> Result<(), String> {
    if (out.len() + data.len() + name.len() + 64) as u64 > max {
        return Err(format!("more than one download carries ({} MB) once packed", max >> 20));
    }
    // Deflated when the caller handed fewer bytes than the file holds.
    let method: u16 = if !dir && data.len() < usize { 8 } else { 0 };
    let offset = out.len() as u32;
    out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0x0800u16.to_le_bytes());
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(&time.to_le_bytes());
    out.extend_from_slice(&date.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&(usize as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(data);
    entries.push(Entry { name, crc, csize: data.len() as u32, usize: usize as u32, method, time, date, mode, offset, dir });
    Ok(())
}

/// MS-DOS time and date (UTC; zip has no zone), from 1980 on.
fn dos_time(t: Option<std::time::SystemTime>) -> (u16, u16) {
    let s = t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(315_532_800).max(315_532_800);
    let (days, rem) = (s / 86_400, s % 86_400);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let time = ((rem / 3600) << 11 | (rem % 3600 / 60) << 5 | (rem % 60) / 2) as u16;
    let date = (((y - 1980) << 9) | (m << 5) | d) as u16;
    (time, date)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_folder_zips_and_unzip_reads_it_back() {
        let d = std::env::temp_dir().join(format!("cove-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("proj/r")).unwrap();
        std::fs::write(d.join("proj/app.R"), "x <- 1\n".repeat(200)).unwrap();
        std::fs::write(d.join("proj/r/log.r"), "#' @export\nf <- 1\n").unwrap();
        std::fs::write(d.join("proj/empty.txt"), "").unwrap();
        let z = super::zip(&d.join("proj"), 1 << 30).unwrap();
        assert!(z.starts_with(b"PK\x03\x04"));
        let f = d.join("proj.zip");
        std::fs::write(&f, &z).unwrap();
        // Read back by the system's unzip where there is one; the bytes and
        // CRCs must check.
        if let Ok(o) = std::process::Command::new("unzip").arg("-t").arg(&f).output() {
            let s = String::from_utf8_lossy(&o.stdout);
            assert!(o.status.success() && s.contains("No errors"), "{s}");
            let l = String::from_utf8_lossy(&std::process::Command::new("unzip").arg("-l").arg(&f).output().unwrap().stdout).into_owned();
            for n in ["proj/", "proj/app.R", "proj/r/log.r", "proj/empty.txt"] {
                assert!(l.contains(n), "{n} missing:\n{l}");
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }
}
