//! Cove on this computer, in a browser (operator, 2026-11-16: where Cove runs
//! - this computer, ndexr's server, their own EC2 - by "desktop or server mode
//! for each of those"). "Open in a browser" starts `cove serve` from this very
//! binary on a free loopback port, with a token of its own, waits until it
//! answers, and opens its sign-in link in the system browser. One per app; it
//! ends with the app (Linux: the kernel's parent-death signal).

use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Running {
    child: Child,
    url: String,
}

static SERVER: Mutex<Option<Running>> = Mutex::new(None);

/// The browser address of this computer's Cove, starting it if it is not
/// running: `http://127.0.0.1:<port>/login?token=<token>`.
pub fn open() -> Result<String, String> {
    let mut slot = SERVER.lock().unwrap();
    if let Some(r) = slot.as_mut() {
        if matches!(r.child.try_wait(), Ok(None)) {
            return Ok(r.url.clone());
        }
    }
    let web = crate::server::default_web_dir();
    if !web.join("index.html").is_file() {
        return Err(format!("this Cove has no browser UI ({} is empty) - update Cove; the download carries it from 0.1.22", web.display()));
    }
    let port = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(|e| format!("no free port: {e}"))?.port();
    let token = crate::host::account::new_token();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(exe);
    cmd.args(["serve", "--listen", &format!("127.0.0.1:{port}"), "--web-dir", &web.display().to_string()])
        .env("COVE_TOKEN", &token)
        // Its own sign-in only: never the cell's loopback trust.
        .env_remove("COVE_TRUST_LOOPBACK")
        .env_remove("COVE_TRUST_EMAILS")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: prctl in the child before exec, async-signal-safe.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn().map_err(|e| format!("could not start cove serve: {e}"))?;
    // Up when its port answers.
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        if std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_millis(300)).is_ok() {
            break;
        }
        if let Ok(Some(st)) = child.try_wait() {
            return Err(format!("cove serve ended at once ({st})"));
        }
        if Instant::now() > until {
            let _ = child.kill();
            return Err("cove serve did not answer within 15 s".into());
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    let url = format!("http://127.0.0.1:{port}/login?token={token}");
    *slot = Some(Running { child, url: url.clone() });
    Ok(url)
}

/// Stop it, when this app started one.
pub fn stop() -> bool {
    match SERVER.lock().unwrap().take() {
        Some(mut r) => {
            let _ = r.child.kill();
            let _ = r.child.wait();
            true
        }
        None => false,
    }
}

/// It is running (the menu offers Stop).
pub fn running() -> bool {
    SERVER.lock().unwrap().as_mut().is_some_and(|r| matches!(r.child.try_wait(), Ok(None)))
}
