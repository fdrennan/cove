//! The build machines, managed from Builds (operator, 2026-10-18: "an admin
//! should be able to, in builds, manage the state of the box like we do in
//! n-boxes, but in the cove application, non admin no - and shouldnt be
//! something that someone can access with the app if on mac or linux desktop
//! app, just admins logged in through ndexr can do so").
//!
//! The same machines and the same commands as n-box.ndexr.io (module.r
//! `.on_box`, `.shutdown_cmd`, `.terminate_cmd`, `.act`): the registry's
//! driven servers (aws.ndexr.io/provision/servers.json, role four-box or
//! n-box, not parked), their run files under /srv/ndexr/host_runs/<role>, and
//! `ndexr aws server start|stop|terminate` plus `ndexr stack up|down` on the
//! box, run as the platform account (its CLI, AWS credentials and ssh key).
//!
//! Only `cove multiuser` answers these, and only for an ndexr admin
//! (multiuser.rs `on_behalf`, host.rs `ndexr_admin`). A person's own session
//! and the desktop app refuse them outright (host.rs), so the controls exist
//! nowhere else.

use std::path::{Path, PathBuf};

use crate::api::BoxView;

const REGISTRY: &str = "/home/ubuntu/ndexr/src/domains/drennanfreddy/aws.ndexr.io/provision/servers.json";
const RUNS: &str = "/srv/ndexr/host_runs";
const STACK_ROOT: &str = "/opt/ndexr/stack";
const DRIVEN: [&str; 2] = ["four-box", "n-box"];
const DESKTOP_PUSH: &str = "/home/ubuntu/ndexr/src/domains/drennanfreddy/cove.ndexr.io/onprem/desktop-push.sh";
const DESKTOP_STATE: &str = "/home/ubuntu/.local/state/cove-desktop";
const WINDOWS_RUN: &str = "/home/ubuntu/ndexr/src/domains/drennanfreddy/aws.ndexr.io/provision/four-box/windows-run.sh";

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    pub(super) struct Server {
        name: String,
        role: String,
        arch: String,
        instance: String,
        parked: bool,
        /// "windows" from the registry's `os`, else Linux.
        os: String,
        /// A LAN machine's ssh alias (the registry's `lan` list): not EC2.
        ssh: Option<String>,
    }

    impl Server {
        fn stem(&self) -> String {
            self.name.strip_prefix(&format!("{}-", self.role)).unwrap_or(&self.name).to_owned()
        }
        fn file(&self, ext: &str) -> PathBuf {
            Path::new(RUNS).join(&self.role).join(format!("{}.{ext}", self.stem()))
        }
        fn triple(&self) -> &'static str {
            if self.arch == "aarch64" { "aarch64-unknown-linux-gnu" } else { "x86_64-pc-linux-gnu" }
        }
    }

    /// The registry's driven servers: an unreadable registry is none.
    fn servers() -> Vec<Server> {
        let v: serde_json::Value = std::fs::read(REGISTRY).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        v["servers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let role = s["role"].as_str()?.to_owned();
                if !DRIVEN.contains(&role.as_str()) {
                    return None;
                }
                // As n-box reads it: the entry's arch, else its AMI's.
                let ami = s["ami_ssm"].as_str().unwrap_or("");
                let arch = s["arch"].as_str().map(str::to_owned).unwrap_or_else(|| if ami.contains("arm64") || ami.contains("aarch64") { "aarch64".into() } else { "x86_64".into() });
                let instance = s["type"].as_str().unwrap_or("").to_owned();
                let os = s["os"].as_str().unwrap_or("linux").to_owned();
                Some(Server { name: s["name"].as_str()?.to_owned(), role, arch, instance, parked: s["parked"].as_bool().unwrap_or(false), os, ssh: None })
            })
            .filter(|s| !s.parked)
            .chain(v["lan"].as_array().into_iter().flatten().filter_map(|s| {
                let role = s["role"].as_str()?.to_owned();
                DRIVEN.contains(&role.as_str()).then_some(())?;
                Some(Server {
                    name: s["name"].as_str()?.to_owned(),
                    role,
                    arch: s["arch"].as_str().unwrap_or("x86_64").to_owned(),
                    instance: "LAN".into(),
                    parked: false,
                    os: s["os"].as_str().unwrap_or("linux").to_owned(),
                    ssh: Some(s["ssh"].as_str().or(s["name"].as_str())?.to_owned()),
                })
            }))
            .collect()
    }

    /// A LAN machine as the desktop push last left it (desktop-push.sh
    /// status: building · ok · needs-action · failed · unreachable).
    fn lan_view(s: &Server) -> BoxView {
        let st: serde_json::Value = std::fs::read(Path::new(DESKTOP_STATE).join(format!("{}.json", s.name)))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let state = match st["state"].as_str().unwrap_or("") {
            "building" => "running",
            "ok" => "done",
            "failed" | "needs-action" => "failed",
            "unreachable" => "off",
            _ => "unknown",
        };
        BoxView {
            server: s.name.clone(),
            role: s.role.clone(),
            stem: s.name.clone(),
            arch: s.arch.clone(),
            instance: s.instance.clone(),
            step: if state == "running" { "desktop Cove".into() } else { st["note"].as_str().unwrap_or("").chars().take(80).collect() },
            step_index: 0,
            steps: 0,
            up: state != "off",
            state: state.into(),
            quiet_s: 0,
        }
    }

    fn view(s: &Server) -> BoxView {
        if s.ssh.is_some() {
            return lan_view(s);
        }
        let st: serde_json::Value = std::fs::read(s.file("json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        let quiet_s = std::fs::metadata(s.file("log"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(i64::MAX, |d| now - d.as_secs() as i64);
        // The one answer every view gives (host.rs box_power): an address
        // file left behind, or a run file from days ago, no longer says "on".
        let (power, state) = crate::host::box_power(&Path::new(RUNS).join(&s.role), &s.role, &s.stem());
        BoxView {
            server: s.name.clone(),
            role: s.role.clone(),
            stem: s.stem(),
            arch: s.arch.clone(),
            instance: s.instance.clone(),
            step: if state == "off" { String::new() } else { st["step"].as_str().unwrap_or("").to_owned() },
            step_index: st["step_index"].as_i64().unwrap_or(0),
            steps: st["steps"].as_array().map_or(0, |a| a.len() as i64),
            up: power != "off",
            state,
            quiet_s,
        }
    }

    pub fn list() -> Vec<BoxView> {
        servers().iter().map(view).collect()
    }

    // ── the commands, as n-box.ndexr.io sends them ─────────────────────────

    const SSH: &str = "ssh -o BatchMode=yes -o ConnectTimeout=15 -o StrictHostKeyChecking=accept-new";
    const WAIT_READY: &str = "for i in $(seq 180); do grep -q 'ndexr-bootstrap: ready' /var/log/ndexr/bootstrap.log 2>/dev/null && break; grep -q 'ndexr-bootstrap: FAILED' /var/log/ndexr/bootstrap.log 2>/dev/null && { echo 'bootstrap FAILED'; exit 1; }; sleep 10; done;";
    const UPDATE: &str = "~/.local/bin/ndexr update --remote || echo 'ndexr update failed - building with the installed one';";
    const POWEROFF: &str = "$(~/.local/bin/ndexr stack up --help 2>/dev/null | grep -q -- --poweroff-when-done && echo --poweroff-when-done)";
    const LOG: &str = "/var/log/ndexr/prefix.log";

    fn q(s: &str) -> String {
        crate::api::shell_quote(s)
    }

    /// Start the machine, show what AWS said in the job's log, and leave its
    /// address in $ip (or fail). The output goes to fd 2 as it is: `tee
    /// /dev/stderr` reopened the log by path, and under `cove multiuser` the job
    /// may write to that descriptor but not open the file - "Asking AWS to start
    /// the machine · /dev/stderr: Permission denied (os error 13)" (operator,
    /// 2026-10-24, the intel box).
    fn start_ip(name: &str) -> String {
        format!(
            "out=$(ndexr aws server start {} 2>&1); printf '%s\\n' \"$out\" >&2; \
             ip=$(printf '%s\\n' \"$out\" | grep -oE '[0-9]+\\.[0-9]+\\.[0-9]+\\.[0-9]+' | tail -n 1) && [ -n \"$ip\" ] &&",
            q(name)
        )
    }

    /// The Windows lane (2026-10-19): no ndexr CLI or stack on the machine.
    /// It is reached as Administrator and builds through
    /// aws.ndexr.io/provision/four-box/windows-run.sh, each build a SYSTEM
    /// scheduled task. Start used the Linux path - ssh ubuntu@ and `ndexr
    /// stack up` - and failed: "ubuntu@...: Permission denied (publickey...)".
    fn on_windows(s: &Server, verb: &str) -> String {
        let run = q(WINDOWS_RUN);
        if verb == "down" {
            return format!(
                "ip=$(cat {}) && {SSH} Administrator@$ip 'schtasks /End /TN ndexr-build-cove; schtasks /End /TN ndexr-build-r-set'",
                q(&s.file("ip").display().to_string())
            );
        }
        // sshd answers a minute or two after the machine reports running
        // (Windows boots slower than its state flips); the runner needs it.
        format!(
            "{} \
             for i in $(seq 60); do {SSH} Administrator@$ip 'exit 0' 2>/dev/null && break; sleep 10; done && \
             {run} cove && {run} r-set",
            start_ip(&s.name)
        )
    }

    #[cfg(test)]
    pub(super) fn test_server(name: &str, os: &str) -> Server {
        Server { name: name.into(), role: "four-box".into(), arch: "x86_64".into(), instance: String::new(), parked: false, os: os.into(), ssh: (os == "macos").then(|| name.to_owned()) }
    }

    #[cfg(test)]
    pub(super) fn on_box_for_test(s: &Server, verb: &str) -> String {
        on_box(s, verb)
    }

    /// The desktop Coves (laptop, neo) on the current Cove release: every
    /// kick-off of a build machine asks for it (operator, 2026-10-20: "when
    /// we build n-box we need to make sure the desktop versions of cove are
    /// also updated"). desktop-push.sh coalesces on the release, so three
    /// machines started together build each desktop once - and none when it
    /// already has this release.
    fn desktops_current() -> String {
        format!("SETTLE=0 {} all \"$(readlink /usr/local/lib/cove/current | sed 's#releases/##')\" ;", q(DESKTOP_PUSH))
    }

    fn on_box(s: &Server, verb: &str) -> String {
        if let Some(host) = &s.ssh {
            // A LAN machine builds the desktop Cove; there is no long run to stop.
            return if verb == "down" {
                format!("echo '{host}: builds the desktop Cove when kicked off - nothing runs to stop'")
            } else {
                format!("SETTLE_ONE=0 {} {} \"by-hand-$(date -u +%Y%m%dT%H%M%SZ)\"", q(DESKTOP_PUSH), q(&s.name))
            };
        }
        if verb == "up" {
            let rest = if s.os == "windows" { on_windows(s, verb) } else { on_linux(s, verb) };
            return format!("{} {rest}", desktops_current());
        }
        if s.os == "windows" {
            return on_windows(s, verb);
        }
        on_linux(s, verb)
    }

    fn on_linux(s: &Server, verb: &str) -> String {
        let stack = format!("{STACK_ROOT}/{}", s.triple());
        let ndexr = format!("~/.local/bin/ndexr stack {verb} --dir {stack}");
        if verb == "down" {
            return format!("ip=$(cat {}) && {SSH} ubuntu@$ip {}", q(&s.file("ip").display().to_string()), q(&ndexr));
        }
        let remote = format!("{WAIT_READY} {UPDATE} {ndexr} --report-errors --follow-releases {POWEROFF} --publish --log {LOG}");
        format!(
            "{} {SSH} ubuntu@$ip {}",
            start_ip(&s.name),
            q(&remote)
        )
    }

    /// Stop or terminate the machine, then write the state it can no longer
    /// report itself, and drop the address that made it count as up.
    fn power(s: &Server, cli: &str, state: &str) -> String {
        let json = q(&s.file("json").display().to_string());
        let tmp = q(&format!("{}.tmp", s.file("json").display()));
        format!(
            "ndexr aws server {cli} {} && printf '{{\"state\":\"{state}\",\"finished\":\"%s\"}}\\n' \"$(date -u +%FT%TZ)\" > {tmp} && mv -f {tmp} {json} && rm -f {}",
            q(&s.name),
            q(&s.file("ip").display().to_string())
        )
    }

    /// The shell for `verb` on `server`: kick off · stop build · shut down · terminate.
    pub fn command(server: &str, verb: &str) -> Result<String, String> {
        let all = servers();
        let s = all.iter().find(|s| s.name == server).ok_or_else(|| format!("{server}: not a build machine in the registry"))?;
        Ok(match verb {
            // A stale box's supervisor may still hold the build: stop it first,
            // so Kick off restarts rather than bouncing off a hung run.
            "up" if view(s).state == "stale" => format!("({}) ; {}", on_box(s, "down"), on_box(s, "up")),
            "up" | "down" => on_box(s, verb),
            "shutdown" | "terminate" if s.ssh.is_some() => return Err(format!("{server} is a LAN machine, not an AWS one - power it from the machine itself")),
            "shutdown" => power(s, "stop", "off"),
            "terminate" => power(s, "terminate --yes", "terminated"),
            _ => return Err(format!("{verb}: not something a box does")),
        })
    }

    /// One job slot per machine, so two machines' commands run side by side.
    fn kind(server: &str) -> &'static str {
        static KINDS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, &'static str>>> = std::sync::OnceLock::new();
        let mut k = KINDS.get_or_init(Default::default).lock().unwrap();
        k.entry(server.to_owned()).or_insert_with(|| Box::leak(format!("box:{server}").into_boxed_str()))
    }

    pub fn act(server: &str, verb: &str) -> Result<crate::api::JobView, String> {
        let cmd = crate::host::as_platform(&command(server, verb)?)?;
        let what = match verb {
            "up" => "kick off",
            "down" => "stop build",
            "shutdown" => "shut down",
            _ => "terminate",
        };
        crate::host::stack::jobs::start(kind(server), format!("{server}: {what}"), cmd, None)
    }

    pub fn job(server: &str) -> crate::api::JobView {
        crate::host::stack::jobs::view(kind(server))
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{act, job, list};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn the_commands_are_n_boxs() {
        // Only when this machine has the registry (the platform host).
        if !Path::new(REGISTRY).is_file() {
            return;
        }
        let Some(b) = list().into_iter().find(|b| b.role == "four-box") else { return };
        let off = native::command(&b.server, "shutdown").unwrap();
        assert!(off.starts_with(&format!("ndexr aws server stop '{}'", b.server)) || off.starts_with(&format!("ndexr aws server stop {}", b.server)), "{off}");
        assert!(off.contains("\"state\":\"off\""));
        let kill = native::command(&b.server, "terminate").unwrap();
        assert!(kill.contains("terminate --yes") && kill.contains("terminated"));
        let up = native::command(&b.server, "up").unwrap();
        assert!(up.contains("ndexr aws server start") && up.contains("stack up --dir /opt/ndexr/stack/") && up.contains("--publish"), "{up}");
        assert!(native::command(&b.server, "format-disk").is_err());
        assert!(native::command("not-a-box", "up").is_err());
    }

    #[test]
    fn a_lan_mac_builds_the_desktop_and_a_kick_off_updates_the_desktops() {
        let neo = super::native::test_server("neo", "macos");
        let up = super::native::on_box_for_test(&neo, "up");
        assert!(up.contains("desktop-push.sh") && up.contains("'neo'") && !up.contains("aws server start"), "{up}");
        let intel = super::native::test_server("four-box-intel", "linux");
        let up = super::native::on_box_for_test(&intel, "up");
        assert!(up.starts_with("SETTLE=0 ") && up.contains("desktop-push.sh") && up.contains(" all ") && up.contains("stack up"), "{up}");
        let down = super::native::on_box_for_test(&intel, "down");
        assert!(!down.contains("desktop-push.sh"), "{down}");
    }

    #[test]
    fn no_command_reopens_stderr_and_every_one_parses() {
        // 2026-10-24: `tee /dev/stderr` failed under cove multiuser with
        // "Permission denied (os error 13)" and the machine never started.
        for (name, os) in [("four-box-intel", "linux"), ("four-box-windows", "windows")] {
            let s = super::native::test_server(name, os);
            for verb in ["up", "down"] {
                let c = super::native::on_box_for_test(&s, verb);
                assert!(!c.contains("/dev/stderr"), "{name} {verb}: {c}");
                let ok = std::process::Command::new("bash").args(["-n", "-c", &c]).status().unwrap().success();
                assert!(ok, "{name} {verb} does not parse: {c}");
            }
        }
    }

    #[test]
    fn a_windows_box_is_started_through_its_own_runner() {
        let s = super::native::test_server("four-box-windows", "windows");
        let up = super::native::on_box_for_test(&s, "up");
        assert!(up.contains("Administrator@$ip") && up.contains("windows-run.sh") && !up.contains("ubuntu@"), "{up}");
        assert!(!up.contains("stack up"), "{up}");
        let down = super::native::on_box_for_test(&s, "down");
        assert!(down.contains("schtasks /End") && !down.contains("ubuntu@"), "{down}");
    }
}
