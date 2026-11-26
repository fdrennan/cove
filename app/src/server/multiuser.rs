//! `cove multiuser` — one server for a machine's people, with what Posit
//! keeps for Workbench rather than open-source RStudio Server (operator,
//! 2026-10-13: "supports all of the things that posit limits in their server
//! version of workbench for multi user management ... multiple sessions up as
//! well as multiple users at one time"). It runs as root; nothing it runs for
//! a person runs as root.
//!
//!   many people at once     each signed in as themselves, each session an
//!                           `cove serve` started as them (uid, gid,
//!                           groups, HOME) on a loopback port
//!   many sessions each      each has its own consoles, settings and plots
//!                           (XDG_CONFIG_HOME / XDG_CACHE_HOME per session) at
//!                           /s/<id>/. There is no list page on a Cove host
//!                           (2026-10-18): a bare visit is the latest session,
//!                           and the others are Cove's toolbar Recent menu
//!                           (recents.rs, /s/<id>/sessions.json)
//!   environments            picked inside Cove (the toolbar's Env), the one
//!                           place for it - not at a New form
//!   sign-in                 `auth = pam`: the machine's accounts (service
//!                           cove, else login), password and account
//!                           rules. `auth = proxy`: a front proxy that already
//!                           signed the person in names them in a header -
//!                           Workbench's proxied auth - trusted only with the
//!                           proxy's secret (`proxy-key-file`), never for being
//!                           on loopback: anyone with a shell here is on it
//!   administration          /admin: every session of every person, Stop,
//!                           Stop all of a person's, limits, the audit log
//!   limits                  `max-sessions-per-user`, `max-sessions`,
//!                           `idle-minutes` (an idle session ends)
//!   audit                   sign-ins, refusals, session start / stop, admin
//!                           actions, one line each (`audit-log`)
//!   lenses                  /?domain=<host> or /?folder=<path> opens the
//!                           person's latest session (a new one if none) on it
//!   RStudio                 only on an rstudio.* host (2026-10-18, operator:
//!                           "cove shouldnt be creating sessions or managing
//!                           rstudio and positron"): Cove's own host neither
//!                           lists nor starts them. A session is a Cove OR an
//!                           RStudio (2026-10-13): `rstudio-launcher` starts one
//!                           Inlet rserver per session as the
//!                           person, so a person has several RStudios at once,
//!                           each in its own folder. They live at /r/<id>/,
//!                           which nginx proxies straight to the session's
//!                           port once /__rstudio_gate has matched the
//!                           session's capability cookie (RStudio needs
//!                           websockets, streaming and its own cookies, which
//!                           `forward` does not carry). A lens on an RStudio
//!                           host opens the person's RStudio already on that
//!                           folder, or starts one there.
//!   Positron                (2026-10-14, operator: "like rstudio.ndexr.io
//!                           and ide.ndexr.io ... positron.ndexr.io to take and
//!                           create a session for the logged in user"):
//!                           `positron-launcher` starts one `ml Positron-Server`
//!                           server per session as the person, at /p/<id>/,
//!                           gated and proxied by nginx exactly as RStudio is.
//!                           Only on a positron.* host, as RStudio.
//!
//! Not here (Workbench has them): load balancing across servers, launching
//! sessions on Kubernetes or Slurm, suspend/resume, project sharing.
//!
//! Policy is /etc/cove/multiuser.conf (or --config), read at start; an
//! unknown key stops the server rather than being ignored.

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tiny_http::{Header, Method, Request, Response, Server};

const COOKIE: &str = "cove_mu";
/// An admin acting as someone (operator, 2026-10-18: "as an administrator I
/// need to be able to open up cove as another user ... so that I can interact
/// as a user"). Names the person; honoured only while the real caller is an
/// admin, and never on /admin, which always answers to the admin themselves.
const ACT_AS: &str = "cove_as";
/// /api/q: a request with its screenshots and recordings (host.rs
/// REQUEST_FILES_MAX, 24 MB, as base64).
const BODY_MAX: u64 = 40 << 20;
/// An upload through a session. nginx allows 15 GB, but forward() holds the
/// body in memory on a machine many people share: 512 MB here; bigger goes
/// through the terminal (scp, curl) or a .tar.gz in pieces.
const UPLOAD_MAX: u64 = 512 << 20;

type Resp = Response<std::io::Cursor<Vec<u8>>>;

// ── configuration ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Auth {
    Pam,
    /// A front proxy names the person in `header`, with `key` proving it is
    /// the proxy speaking.
    Proxy { header: String, key: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Conf {
    pub auth: Auth,
    /// Groups whose members may sign in; empty = any account at or above min-uid.
    pub allow_groups: Vec<String>,
    /// Groups, and names, that see /admin.
    pub admin_groups: Vec<String>,
    pub admin_users: Vec<String>,
    /// A person whose sessions run as another account (`drennanfreddy:ubuntu`:
    /// the operator edits the platform's repository as its owner).
    pub run_as: HashMap<String, String>,
    pub min_uid: u32,
    pub idle: Duration,
    pub ports: (u16, u16),
    pub max_per_user: usize,
    /// The script that starts one RStudio session (onprem/rstudio-session.sh,
    /// installed root-owned); unset = no RStudio sessions.
    pub rstudio_launcher: Option<PathBuf>,
    /// The script that starts one Positron session (onprem/positron-session.sh,
    /// installed root-owned); unset = no Positron sessions.
    pub positron_launcher: Option<PathBuf>,
    pub max_total: usize,
    pub audit_log: PathBuf,
    /// What one account may use, across all its sessions and what they start
    /// (limits.rs): person-memory, person-cpus, person-tasks.
    pub person: crate::host::limits::Limits,
}

impl Default for Conf {
    fn default() -> Self {
        Conf {
            auth: Auth::Pam,
            allow_groups: Vec::new(),
            admin_groups: vec!["sudo".into(), "wheel".into(), "cove-admin".into()],
            admin_users: Vec::new(),
            run_as: HashMap::new(),
            min_uid: 1000,
            idle: Duration::from_secs(120 * 60),
            ports: (32100, 32399),
            max_per_user: 5,
            rstudio_launcher: None,
            positron_launcher: None,
            max_total: 200,
            audit_log: PathBuf::from("/var/log/cove/audit.log"),
            person: crate::host::limits::Limits::NONE,
        }
    }
}

fn list(v: &str) -> Vec<String> {
    v.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// `key = value` lines, `#` comments. Strict: a typo must never silently
/// widen who may sign in.
pub fn parse_conf(text: &str) -> Result<Conf, String> {
    let mut c = Conf::default();
    let (mut auth, mut header, mut key_file) = ("pam".to_owned(), "X-Remote-User".to_owned(), String::new());
    for (n, l) in text.lines().enumerate() {
        let l = l.split('#').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        let at = |m: &str| format!("line {}: {m}", n + 1);
        let (k, v) = l.split_once('=').ok_or_else(|| at(&format!("not key = value: {l}")))?;
        let (k, v) = (k.trim(), v.trim());
        let num = |v: &str| v.parse::<u64>().map_err(|_| at(&format!("{k} is a number")));
        match k {
            "auth" => auth = v.to_owned(),
            "proxy-header" => header = v.to_owned(),
            "proxy-key-file" => key_file = v.to_owned(),
            "allow-groups" => c.allow_groups = list(v),
            "admin-groups" => c.admin_groups = list(v),
            "admin-users" => c.admin_users = list(v),
            "run-as" => {
                for pair in list(v) {
                    let (a, b) = pair.split_once(':').ok_or_else(|| at("run-as is person:account, ..."))?;
                    if !valid_user(a) || !valid_user(b) || b == "root" {
                        return Err(at(&format!("run-as {pair}: two login names, never root")));
                    }
                    c.run_as.insert(a.to_owned(), b.to_owned());
                }
            }
            "min-uid" => c.min_uid = num(v)? as u32,
            "idle-minutes" => c.idle = Duration::from_secs(60 * num(v)?.max(1)),
            "max-sessions-per-user" => c.max_per_user = num(v)?.max(1) as usize,
            "max-sessions" => c.max_total = num(v)?.max(1) as usize,
            "audit-log" => c.audit_log = PathBuf::from(v),
            "person-memory" => c.person.memory = crate::host::limits::parse_size(v).ok_or_else(|| at("person-memory is a size: 6G, 512M"))?,
            "person-cpus" => c.person.cpus = num(v)? as u32,
            "person-tasks" => c.person.tasks = num(v)? as u32,
            "rstudio-launcher" | "positron-launcher" => {
                // Run as every person, so only root may be able to change it.
                let meta = std::fs::metadata(v).map_err(|e| at(&format!("{k} {v}: {e}")))?;
                {
                    use std::os::unix::fs::MetadataExt;
                    if meta.uid() != 0 || meta.mode() & 0o022 != 0 {
                        return Err(at(&format!("{k} {v}: must be root-owned and not group/world-writable")));
                    }
                }
                if k == "rstudio-launcher" {
                    c.rstudio_launcher = Some(PathBuf::from(v));
                } else {
                    c.positron_launcher = Some(PathBuf::from(v));
                }
            }
            "ports" => {
                let (a, b) = v.split_once('-').ok_or_else(|| at("ports is lo-hi"))?;
                let (a, b): (u16, u16) = (a.trim().parse().map_err(|_| at("ports: lo"))?, b.trim().parse().map_err(|_| at("ports: hi"))?);
                if a < 1024 || b < a {
                    return Err(at(&format!("ports {a}-{b}: above 1024, lo before hi")));
                }
                c.ports = (a, b);
            }
            _ => return Err(at(&format!("unknown setting {k}"))),
        }
    }
    c.auth = match auth.as_str() {
        "pam" => Auth::Pam,
        "proxy" => {
            if key_file.is_empty() {
                return Err("auth = proxy needs proxy-key-file: a header alone can be sent by anyone with a shell on this machine".into());
            }
            let key = std::fs::read_to_string(&key_file).map_err(|e| format!("proxy-key-file {key_file}: {e}"))?.trim().to_owned();
            if key.len() < 32 {
                return Err(format!("proxy-key-file {key_file}: at least 32 characters"));
            }
            Auth::Proxy { header, key }
        }
        other => return Err(format!("auth = {other}: pam or proxy")),
    };
    Ok(c)
}

// ── accounts ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Account {
    name: String,
    uid: u32,
    gid: u32,
    home: String,
    shell: String,
    groups: Vec<String>,
}

/// A login name as useradd allows it; nothing else reaches PAM or getpwnam.
pub fn valid_user(u: &str) -> bool {
    !u.is_empty() && u.len() <= 32 && u.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() && !c.is_ascii_uppercase() || c == '_') && u.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-.".contains(c))
}

fn account(name: &str) -> Option<Account> {
    let c = CString::new(name).ok()?;
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 16 << 10];
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe { libc::getpwnam_r(c.as_ptr(), &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) };
    if rc != 0 || out.is_null() {
        return None;
    }
    let s = |p: *const libc::c_char| if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() };
    let groups = Command::new("id").args(["-Gn", name]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).split_whitespace().map(str::to_owned).collect()).unwrap_or_default();
    Some(Account { name: name.to_owned(), uid: pw.pw_uid, gid: pw.pw_gid, home: s(pw.pw_dir), shell: s(pw.pw_shell), groups })
}

fn in_any(a: &Account, groups: &[String]) -> bool {
    a.groups.iter().any(|g| groups.contains(g))
}

/// Who may come in: a real account (not a system one), with a login shell,
/// in an allowed group when there are any.
fn may_sign_in(a: &Account, c: &Conf) -> Result<(), String> {
    if a.uid < c.min_uid {
        return Err("system accounts do not sign in here".into());
    }
    if a.shell.ends_with("nologin") || a.shell.ends_with("/false") {
        return Err("this account has no login shell".into());
    }
    if !c.allow_groups.is_empty() && !in_any(a, &c.allow_groups) {
        return Err(format!("not in a group allowed to sign in ({})", c.allow_groups.join(", ")));
    }
    Ok(())
}

// ── PAM, loaded at run time ─────────────────────────────────────────────────

mod pam {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_int, c_void};

    #[repr(C)]
    struct Message {
        style: c_int,
        msg: *const c_char,
    }
    #[repr(C)]
    struct Reply {
        resp: *mut c_char,
        retcode: c_int,
    }
    #[repr(C)]
    struct Conv {
        conv: extern "C" fn(c_int, *mut *const Message, *mut *mut Reply, *mut c_void) -> c_int,
        data: *mut c_void,
    }
    type Start = unsafe extern "C" fn(*const c_char, *const c_char, *const Conv, *mut *mut c_void) -> c_int;
    type Step = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
    type End = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
    type Strerror = unsafe extern "C" fn(*mut c_void, c_int) -> *const c_char;

    const SUCCESS: c_int = 0;
    const PROMPT_ECHO_OFF: c_int = 1;
    const PROMPT_ECHO_ON: c_int = 2;
    const SILENT: c_int = 0x8000;
    const DISALLOW_NULL_AUTHTOK: c_int = 0x0001;

    struct Creds {
        user: CString,
        pass: CString,
    }

    /// Answers PAM's questions: the name for an echoed prompt, the password
    /// for a hidden one, nothing for informational messages. PAM frees what
    /// this allocates.
    extern "C" fn converse(n: c_int, msgs: *mut *const Message, out: *mut *mut Reply, data: *mut c_void) -> c_int {
        if n <= 0 || msgs.is_null() || out.is_null() || data.is_null() {
            return 19; // PAM_CONV_ERR
        }
        let creds = unsafe { &*(data as *const Creds) };
        let replies = unsafe { libc::calloc(n as usize, std::mem::size_of::<Reply>()) } as *mut Reply;
        if replies.is_null() {
            return 5; // PAM_BUF_ERR
        }
        for i in 0..n as usize {
            let m = unsafe { &**msgs.add(i) };
            let text = match m.style {
                PROMPT_ECHO_OFF => Some(&creds.pass),
                PROMPT_ECHO_ON => Some(&creds.user),
                _ => None,
            };
            if let Some(t) = text {
                unsafe { (*replies.add(i)).resp = libc::strdup(t.as_ptr()) };
            }
        }
        unsafe { *out = replies };
        SUCCESS
    }

    /// libpam: the system's, else the ndexr stack's Linux-PAM. `cove
    /// multiuser` from the stack (`ml cove`, setup-server's unit) runs under
    /// the stack's own loader, which never looks in the host's /usr/lib, so
    /// "libpam.so.0" was not found there and nobody could sign in (operator's
    /// new server, 2026-11-26: "this needs to be built through cvmfs if its
    /// not there"). The stack's libpam carries its own RPATH, so loading it by
    /// its path is enough; it reads the host's /etc/pam.d and loads modules
    /// from its own lib/security.
    fn open_libpam() -> Result<*mut c_void, String> {
        let mut tried = vec!["libpam.so.0".to_owned()];
        if let Ok(root) = std::env::var("EBROOTLINUXMINPAM") {
            tried.push(format!("{root}/lib/libpam.so.0"));
        }
        let arch = std::env::consts::ARCH;
        let mut stack: Vec<String> = std::fs::read_dir("/opt/ndexr/stack")
            .into_iter()
            .flatten()
            .flatten()
            .filter(|t| t.file_name().to_string_lossy().starts_with(arch))
            .flat_map(|t| std::fs::read_dir(t.path().join("eb/software/Linux-PAM")).into_iter().flatten().flatten().map(|v| v.path().join("lib/libpam.so.0").display().to_string()))
            .collect();
        stack.sort();
        tried.extend(stack.into_iter().rev());
        for t in &tried {
            let Ok(c) = CString::new(t.as_str()) else { continue };
            let lib = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW) };
            if !lib.is_null() {
                return Ok(lib);
            }
        }
        Err(format!("no libpam here - neither this machine's nor the ndexr stack's (tried {}); mount the stack, or install the system's PAM", tried.join(", ")))
    }

    fn sym<T>(lib: *mut c_void, name: &str) -> Result<T, String> {
        let c = CString::new(name).unwrap();
        let p = unsafe { libc::dlsym(lib, c.as_ptr()) };
        if p.is_null() {
            return Err(format!("libpam has no {name}"));
        }
        Ok(unsafe { std::mem::transmute_copy(&p) })
    }

    /// Authenticate `user` with `pass` through `service`, then its account
    /// rules (expired, locked). The error is PAM's own word for why.
    pub fn check(service: &str, user: &str, pass: &str) -> Result<(), String> {
        let lib = open_libpam()?;
        let start: Start = sym(lib, "pam_start")?;
        let authenticate: Step = sym(lib, "pam_authenticate")?;
        let acct: Step = sym(lib, "pam_acct_mgmt")?;
        let end: End = sym(lib, "pam_end")?;
        let strerror: Strerror = sym(lib, "pam_strerror")?;
        let creds = Box::new(Creds { user: CString::new(user).map_err(|_| "bad name")?, pass: CString::new(pass).map_err(|_| "bad password")? });
        let conv = Conv { conv: converse, data: &*creds as *const Creds as *mut c_void };
        let svc = CString::new(service).unwrap();
        let mut h: *mut c_void = std::ptr::null_mut();
        let rc = unsafe { start(svc.as_ptr(), creds.user.as_ptr(), &conv, &mut h) };
        if rc != SUCCESS || h.is_null() {
            return Err(format!("pam_start failed ({rc})"));
        }
        let mut rc = unsafe { authenticate(h, SILENT | DISALLOW_NULL_AUTHTOK) };
        if rc == SUCCESS {
            rc = unsafe { acct(h, SILENT | DISALLOW_NULL_AUTHTOK) };
        }
        let why = if rc == SUCCESS { String::new() } else { unsafe { CStr::from_ptr(strerror(h, rc)) }.to_string_lossy().into_owned() };
        unsafe { end(h, rc) };
        drop(creds);
        if rc == SUCCESS {
            Ok(())
        } else {
            Err(why)
        }
    }
}

fn pam_service() -> &'static str {
    if Path::new("/etc/pam.d/cove").is_file() {
        "cove"
    } else {
        "login"
    }
}

// ── sessions ────────────────────────────────────────────────────────────────

/// What a session is: Cove, an RStudio or a Positron.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Ide,
    RStudio,
    Positron,
}

impl Kind {
    fn parse(v: &str) -> Kind {
        if v.eq_ignore_ascii_case("rstudio") {
            Kind::RStudio
        } else if v.eq_ignore_ascii_case("positron") {
            Kind::Positron
        } else {
            Kind::Ide
        }
    }
    fn label(self) -> &'static str {
        match self {
            Kind::Ide => "Cove",
            // ndexr's modified RStudio is Inlet (2026-10-28): Posit's name is
            // not ours to put on it. The key and the /r/<id>/ route stay.
            Kind::RStudio => "Inlet",
            Kind::Positron => "Positron",
        }
    }
    /// The form value New sends.
    fn key(self) -> &'static str {
        match self {
            Kind::Ide => "ide",
            Kind::RStudio => "rstudio",
            Kind::Positron => "positron",
        }
    }
    /// Reached by nginx straight on the session's port, past /__rstudio_gate,
    /// rather than relayed by `forward`.
    fn direct(self) -> bool {
        self != Kind::Ide
    }
    /// Where the session lives: /s/<id>/ through `forward`; /r/<id>/ and
    /// /p/<id>/ through nginx straight to the session's server.
    fn url(self, id: &str) -> String {
        match self {
            Kind::Ide => format!("/s/{id}/"),
            Kind::RStudio => format!("/r/{id}/"),
            Kind::Positron => format!("/p/{id}/"),
        }
    }
}

struct Session {
    id: String,
    kind: Kind,
    /// Who signed in (and owns it), and the account it runs as.
    owner: String,
    account: Account,
    name: String,
    module: String,
    project: String,
    port: u16,
    /// ide: the session's own sign-in token. RStudio, Positron: the capability
    /// cookie /__rstudio_gate matches before nginx lets a request through
    /// (Positron's own connection token too).
    token: String,
    child: Proc,
    /// The build it runs (releases/<id>): a session stays on the Cove it
    /// started with until its person asks for the new one (`/update`).
    release: String,
    started: SystemTime,
    last: Instant,
}

/// A session's process: started by this server, or - after a restart of it -
/// found still running and taken back (`adopt`). An adopted one is not our
/// child, so it is watched by its pid.
enum Proc {
    Child(Child),
    Adopted(i32),
}

impl Proc {
    fn pid(&self) -> i32 {
        match self {
            Proc::Child(c) => c.id() as i32,
            Proc::Adopted(p) => *p,
        }
    }
    fn running(&mut self) -> bool {
        match self {
            Proc::Child(c) => matches!(c.try_wait(), Ok(None)),
            // Alive and not a zombie waiting on someone else to reap it.
            Proc::Adopted(p) => {
                let there = unsafe { libc::kill(*p, 0) } == 0;
                let zombie = std::fs::read_to_string(format!("/proc/{p}/stat")).is_ok_and(|t| t.rsplit(')').next().is_some_and(|r| r.trim_start().starts_with('Z')));
                there && !zombie
            }
        }
    }
    fn reap(&mut self) {
        if let Proc::Child(c) = self {
            let _ = c.wait();
        }
    }
}

struct Signin {
    user: String,
    expires: Instant,
}

struct Ctx {
    conf: Conf,
    web: PathBuf,
    exe: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
    cookies: Mutex<HashMap<String, Signin>>,
    /// Failed sign-ins per name: (count, first in this window).
    failures: Mutex<HashMap<String, (u32, Instant)>>,
    /// The stack's modules for the New session form, asked once a while.
    modules: Mutex<Option<(Instant, crate::api::Modules)>>,
    audit: Mutex<()>,
    /// Where each account's sessions are placed and limited (limits.rs
    /// `people_root`); None = this server's cgroup is not delegated.
    people: Option<PathBuf>,
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut r| r.read_exact(&mut b));
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn port_free(p: u16) -> bool {
    TcpStream::connect_timeout(&([127, 0, 0, 1], p).into(), Duration::from_millis(100)).is_err()
}

fn now_utc() -> String {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (d, s) = (t / 86400, t % 86400);
    // days since 1970-01-01 to a civil date (Howard Hinnant's algorithm)
    let z = d as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

/// One line per event, appended by root: who, what, about whom.
fn audit(ctx: &Ctx, who: &str, what: &str, detail: &str) {
    let line = format!("{} {who} {what} {}\n", now_utc(), detail.replace('\n', " "));
    print!("multiuser: {line}");
    let _g = ctx.audit.lock().unwrap();
    if let Some(d) = ctx.conf.audit_log.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&ctx.conf.audit_log) {
        use std::os::unix::fs::PermissionsExt;
        let _ = f.set_permissions(std::fs::Permissions::from_mode(0o640));
        let _ = f.write_all(line.as_bytes());
    }
}

fn audit_tail(ctx: &Ctx, n: usize) -> Vec<String> {
    let t = std::fs::read_to_string(&ctx.conf.audit_log).unwrap_or_default();
    let v: Vec<String> = t.lines().map(str::to_owned).collect();
    v[v.len().saturating_sub(n)..].iter().rev().cloned().collect()
}

fn modules(ctx: &Ctx) -> crate::api::Modules {
    let mut m = ctx.modules.lock().unwrap();
    if let Some((at, v)) = m.as_ref() {
        if at.elapsed() < Duration::from_secs(300) {
            return v.clone();
        }
    }
    let v = crate::host::modules();
    *m = Some((Instant::now(), v.clone()));
    v
}

/// The newest env/ module (R + Python + site library), or none.
fn default_module(ctx: &Ctx) -> String {
    modules(ctx).env.first().cloned().unwrap_or_default()
}

fn alive(s: &mut Session) -> bool {
    s.child.running()
}

// ── surviving a restart of this server ──────────────────────────────────────
// Every build of the working tree restarted this server, and the server took
// every session down with it: R, Python, the terminal, the person's place -
// gone on each save (operator, 2026-10-18: "we can't be kicking people out of
// their sessions whenever we restart"). Now the sessions are written down as
// they change, the server going down leaves them running (the unit is
// KillMode=process), and the next server takes them back: same id, port and
// token, so an open page carries on after a few seconds of failed polls.

/// Where the live sessions are written: root's, never a person's.
fn state_file() -> PathBuf {
    PathBuf::from(std::env::var("COVE_STATE_DIR").unwrap_or_else(|_| "/var/lib/cove".into())).join("sessions.json")
}

fn save_sessions(ctx: &Ctx) {
    let rows: Vec<serde_json::Value> = ctx
        .sessions
        .lock()
        .unwrap()
        .values()
        .map(|s| {
            serde_json::json!({
                "id": s.id, "kind": s.kind.key(), "owner": s.owner, "account": s.account.name,
                "name": s.name, "module": s.module, "project": s.project, "port": s.port,
                "token": s.token, "pid": s.child.pid(), "release": s.release,
                "started": s.started.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
            })
        })
        .collect();
    let f = state_file();
    let _ = std::fs::create_dir_all(f.parent().unwrap_or(Path::new("/")));
    let tmp = f.with_extension("tmp");
    // Tokens are in it: root only.
    let ok = std::fs::write(&tmp, serde_json::to_vec(&rows).unwrap_or_default()).is_ok();
    if ok {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::rename(&tmp, &f);
    }
}

/// Take back the sessions the last server left running: each whose process
/// is still there, still runs as its account, and still answers on its port.
fn adopt(ctx: &Ctx) -> usize {
    let Ok(text) = std::fs::read_to_string(state_file()) else { return 0 };
    let rows: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
    let mut n = 0;
    for r in rows {
        let str_ = |k: &str| r[k].as_str().unwrap_or("").to_owned();
        let (id, pid, port) = (str_("id"), r["pid"].as_i64().unwrap_or(0) as i32, r["port"].as_u64().unwrap_or(0) as u16);
        let Some(a) = account(&str_("account")) else { continue };
        let mut child = Proc::Adopted(pid);
        let uid_ok = std::fs::read_to_string(format!("/proc/{pid}/status")).is_ok_and(|t| t.lines().any(|l| l.starts_with("Uid:") && l.split_whitespace().nth(1) == Some(&a.uid.to_string())));
        if pid <= 1 || id.is_empty() || !child.running() || !uid_ok || port_free(port) {
            continue;
        }
        let started = UNIX_EPOCH + Duration::from_secs(r["started"].as_u64().unwrap_or(0));
        let s = Session {
            id: id.clone(),
            kind: Kind::parse(&str_("kind")),
            owner: str_("owner"),
            account: a,
            name: str_("name"),
            module: str_("module"),
            project: str_("project"),
            port,
            token: str_("token"),
            child,
            release: str_("release"),
            started,
            last: Instant::now(),
        };
        audit(ctx, "server", "session-adopt", &format!("{id} \"{}\" of {} on :{port} (release {})", s.name, s.owner, if s.release.is_empty() { "-" } else { &s.release }));
        ctx.sessions.lock().unwrap().insert(id, s);
        n += 1;
    }
    save_sessions(ctx);
    n
}

// ── releases ────────────────────────────────────────────────────────────────
// onprem/run.sh publishes each build to <root>/releases/<stamp>/{cove,web} and
// points <root>/current at it. A session starts on whatever `current` is then
// and keeps that build; the page asks /s/<id>/release.json and offers the
// person the newer one, in their own time.

/// The published build sessions start on: (its id, binary, web dir). Without
/// a releases layout (a .deb, the desktop) it is this server's own, unnamed.
fn current_release(ctx: &Ctx) -> (String, PathBuf, PathBuf) {
    let root = std::env::var_os("COVE_RELEASES_ROOT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/usr/local/lib/cove"));
    if let Ok(dir) = std::fs::canonicalize(root.join("current")) {
        let (exe, web) = (dir.join("cove"), dir.join("web"));
        if exe.is_file() && web.join("index.html").is_file() {
            let id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return (id, exe, web);
        }
    }
    (String::new(), ctx.exe.clone(), ctx.web.clone())
}

/// The account a person's sessions run as: their own, unless run-as says.
fn account_for(ctx: &Ctx, user: &str) -> Result<Account, String> {
    let as_ = ctx.conf.run_as.get(user).map(String::as_str).unwrap_or(user);
    let a = account(as_).ok_or_else(|| format!("no account {as_} on this machine"))?;
    may_sign_in(&a, &ctx.conf)?;
    if a.uid == 0 {
        return Err("root does not get a session".into());
    }
    Ok(a)
}

/// Where a person may start a session: their domains only (operator,
/// 2026-10-14: "session selector at cove.ndexr.io should have the users
/// domains only as options of where to start"). A person's domains are the
/// folders named for a host in their own checkout (~/domains/<host>, or
/// ~/domains/<them>/<host>) and, on the platform's own checkout, under their
/// owner folder (~/ndexr/src/domains/<them>/<host> - owner folders are ndexr
/// usernames). Read as root; only folder names and marker files are looked at.
fn person_domains(user: &str, a: &Account) -> Vec<(String, String)> {
    let home = Path::new(&a.home);
    let mut roots = vec![home.join("domains").join(user), home.join("ndexr/src/domains").join(user)];
    // A cell-style checkout holds only the person's own domains, flat.
    roots.push(home.join("domains"));
    let mut out: Vec<(String, String)> = Vec::new();
    for root in roots {
        for e in std::fs::read_dir(&root).into_iter().flatten().flatten() {
            if let Some(d) = crate::host::domain_at(&e.path()) {
                if !out.iter().any(|(h, _)| *h == d.host) {
                    out.push((d.host, d.path));
                }
            }
        }
    }
    out.sort();
    out
}

/// The domains a person may work on: their own and those shared with them to
/// work on (shares.rs - its folder at ~/shared/<host>, its agent, its thread).
/// Archive, access, deploy, versions and images stay `person_domains`: the
/// owner's alone.
fn workable_domains(user: &str, a: &Account) -> Vec<(String, String)> {
    let mut out = person_domains(user, a);
    for (h, p, _) in crate::host::shares::granted(Path::new(&a.home)) {
        if !out.iter().any(|(x, _)| *x == h) {
            out.push((h, p));
        }
    }
    out
}

/// Wakes the shares loop (`run`) now rather than in up to 20 s.
static SHARES_KICK: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

/// shares.rs reconcile, as root, with what it changed in the audit log.
fn shares_reconcile(ctx: &Ctx) {
    let acct = |u: &str| account(u).filter(|a| a.uid >= 1000).map(|a| (PathBuf::from(a.home), a.uid, a.gid));
    match crate::host::shares::reconcile(acct) {
        Ok(said) => {
            for l in said {
                audit(ctx, "server", "share", &l);
            }
        }
        Err(e) => eprintln!("multiuser: shares: {e}"),
    }
}

/// app/assets/deploy-app.sh, compiled in (see Query::DeployApp). Inside app/
/// on purpose: the CLI packs only app/ as the source every box builds `ml cove`
/// from, and an include from ../../onprem broke that build on every box
/// ("couldn't read src/../../onprem/deploy-app.sh", request #49, 2026-10-20).
const DEPLOY_APP: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/deploy-app.sh"));

/// One job slot per domain: jobs are keyed by a static name.
fn deploy_kind(host: &str) -> &'static str {
    static KINDS: std::sync::OnceLock<Mutex<HashMap<String, &'static str>>> = std::sync::OnceLock::new();
    let mut m = KINDS.get_or_init(Default::default).lock().unwrap();
    m.entry(host.to_owned()).or_insert_with(|| Box::leak(format!("deploy {host}").into_boxed_str()))
}

/// app/assets/route-app.sh, compiled in (Query::AppRoute): a published
/// domain to its app's APP_PORT - the one step of Up that needs root.
const ROUTE_APP: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/route-app.sh"));

/// app/assets/release-project.sh, compiled in (Query::ProjectRelease): the
/// name of a project Archive has already removed, given up - registry,
/// ownership, vhost.
const RELEASE_PROJECT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/release-project.sh"));

/// app/assets/set-access.sh, compiled in (Query::AppAccess): who may open a
/// domain, in the registry and, for an app on a port, in its vhost's gate.
const SET_ACCESS: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/set-access.sh"));

/// Who may open `host` now, and whether its own process serves it.
fn app_access_read(host: &str) -> Result<crate::api::AppAccessView, String> {
    let out = crate::host::platform_psql_vars(
        "select coalesce((select access_level from domain_registry where domain = :'host' limit 1), 'public')",
        &[("host", host)],
    )?;
    let level = String::from_utf8_lossy(&out).trim().to_owned();
    let level = if ["public", "login", "owner", "admin"].contains(&level.as_str()) { level } else { "public".to_owned() };
    let vhost = std::fs::read_to_string(format!("/home/ubuntu/ndexr/nginx/sites-enabled/{host}")).unwrap_or_default();
    Ok(crate::api::AppAccessView { level, port: vhost.contains("# ndexr-target: port:"), said: String::new() })
}

/// A page password, as route.ndexr.io sets one (src/r/users/site_password.r):
/// the row in site_passwords - sha256 of salt and password, hex - and a
/// vhost that checks it. One SQL statement, sent on psql's stdin with the
/// password dollar-quoted, so it is never in a process list. A vhost that
/// does not check it yet is repaired through the platform's own R
/// (site_password_repair): a stored password nothing checks left four
/// sites open on 2026-07-30. An empty password removes it.
fn site_password(host: &str, password: &str, by: &str) -> Result<String, String> {
    if !crate::host::is_host_name(host) {
        return Err(format!("{host}: not a domain name"));
    }
    if password.is_empty() {
        crate::host::platform_psql_vars("delete from site_passwords where domain = :'host'", &[("host", host)])?;
        return Ok(format!("{host} has no page password now"));
    }
    if password.chars().count() < 4 {
        return Err("a page password needs at least 4 characters".into());
    }
    // A quote tag the password cannot contain.
    let tag = loop {
        let t = format!("cove{}", &crate::host::account::new_token()[..12]);
        if !password.contains(&t) {
            break t;
        }
    };
    let salt = &crate::host::account::new_token()[..32];
    let sql = format!(
        "insert into site_passwords (domain, pass_hash, salt, set_by, updated_at) \
         values (:'host', encode(sha256(convert_to(:'salt' || ${tag}${password}${tag}$, 'UTF8')), 'hex'), :'salt', :'by', now()) \
         on conflict (domain) do update set pass_hash = excluded.pass_hash, salt = excluded.salt, set_by = excluded.set_by, updated_at = now()"
    );
    crate::host::platform_psql_vars(&sql, &[("host", host), ("salt", salt), ("by", by)])?;
    let vhost = std::fs::read_to_string(format!("/home/ubuntu/ndexr/nginx/sites-enabled/{host}")).unwrap_or_default();
    let checked = vhost.lines().any(|l| l.contains("include") && l.contains("conf.d/site_password.conf")) && vhost.contains("auth_request /__site_password_check");
    if checked {
        return Ok(format!("{host} asks for its page password now"));
    }
    site_password_repair(host)
}

/// The vhost made to ask for the page password (site_password_repair).
fn site_password_repair(host: &str) -> Result<String, String> {
    let r = format!(
        "options(box.path = '/home/ubuntu/ndexr/src'); box::use(sp = r/users/site_password); x <- sp$site_password_repair('{host}'); cat(if (isTRUE(x$ok)) 'ok' else 'failed', x$message, '\\n')"
    );
    let out = Command::new("runuser")
        .args(["-u", "ubuntu", "--", "bash", "-c"])
        .arg(format!(
            "cd /home/ubuntu/ndexr; set -a; . ./.env; set +a; case \"${{POSTGRES_HOST:-}}\" in postgres|pgbouncer) export POSTGRES_HOST=127.0.0.1 ;; esac; \
             export NDEXR_HOST_LOCAL=1 LANG=C.UTF-8 R_LIBS=/opt/ndexr/eblib/x86_64-pc-linux-gnu/R-4.6.1-foss-2024a; \
             set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; module load R/4.6.1-foss-2024a >/dev/null 2>&1; Rscript --no-save -e {} 2>&1 | grep -v 'pool>' | tail -2",
            crate::api::shell_quote(&r)
        ))
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if text.starts_with("ok") {
        Ok(format!("{host} asks for its page password now (its vhost was made to check it)"))
    } else {
        Err(format!("the password is stored but {host}'s vhost does not check it yet: {text}"))
    }
}

fn app_access_set(host: &str, level: &str) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    // A Shiny module's gate is the registry row alone (shell.r reads it on
    // each render): one UPDATE, in milliseconds, with registry.r
    // set_access_level's own guards - an owner-only site needs an owner, and
    // the sign-in host stays public. The R route took ~9 s before doing
    // anything (the stack, R, then 4 s of box::use), so the choice sat
    // greyed out (operator, 2026-11-17). An app on its own port also has its
    // vhost's gate rewritten and nginx reloaded: that keeps the R writer below.
    let vhost = std::fs::read_to_string(format!("/home/ubuntu/ndexr/nginx/sites-enabled/{host}")).unwrap_or_default();
    if !vhost.contains("# ndexr-target: port:") {
        let out = crate::host::platform_psql_vars(
            "update domain_registry set access_level = :'lvl' where domain = :'host' \
               and (:'lvl' <> 'owner' \
                    or exists (select 1 from user_domains where lower(domain) = lower(:'host')) \
                    or exists (select 1 from domain_registry where domain = :'host' and coalesce(owner_email, '') <> '')) \
               and (:'host' <> 'console.ndexr.io' or :'lvl' = 'public') \
             returning 1",
            &[("host", host), ("lvl", level)],
        )?;
        return if String::from_utf8_lossy(&out).trim() == "1" {
            Ok(format!("{host} is now {level}"))
        } else if level == "owner" {
            Err(format!("{host} has no owner on record, so only-you would admit nobody"))
        } else {
            Err(format!("the registry has no row for {host}, or it refused '{level}'"))
        };
    }
    let d = Path::new("/run/cove");
    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    let script = d.join("set-access.sh");
    std::fs::write(&script, SET_ACCESS).map_err(|e| format!("{}: {e}", script.display()))?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let out = Command::new("bash").arg(&script).args([host, level]).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() { Ok(text) } else { Err(if text.is_empty() { String::from_utf8_lossy(&out.stderr).trim().to_owned() } else { text }) }
}

/// One Archive job slot per person.
fn archive_kind(user: &str) -> &'static str {
    static KINDS: std::sync::OnceLock<Mutex<HashMap<String, &'static str>>> = std::sync::OnceLock::new();
    let mut m = KINDS.get_or_init(Default::default).lock().unwrap();
    m.entry(user.to_owned()).or_insert_with(|| Box::leak(format!("archive {user}").into_boxed_str()))
}

fn release_project(host: &str, user: &str, admin: bool) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    if !valid_user(user) || !crate::host::is_host_name(host) {
        return Err("not a person or a domain".into());
    }
    let d = Path::new("/run/cove");
    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    let script = d.join("release-project.sh");
    std::fs::write(&script, RELEASE_PROJECT).map_err(|e| format!("{}: {e}", script.display()))?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let out = Command::new("bash").arg(&script).args([host, user, if admin { "1" } else { "0" }]).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() { Ok(text) } else { Err(if text.is_empty() { String::from_utf8_lossy(&out.stderr).trim().to_owned() } else { text }) }
}

fn route_app(host: &str, user: &str, dir: &str) -> Result<String, String> {
    use std::os::unix::fs::PermissionsExt;
    if !valid_user(user) || !crate::host::is_host_name(host) {
        return Err("not a person or a domain".into());
    }
    let d = Path::new("/run/cove");
    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    let script = d.join("route-app.sh");
    std::fs::write(&script, ROUTE_APP).map_err(|e| format!("{}: {e}", script.display()))?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let out = Command::new("bash").arg(&script).args([host, user, dir]).output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if out.status.success() { Ok(text) } else { Err(if text.is_empty() { String::from_utf8_lossy(&out.stderr).trim().to_owned() } else { text }) }
}

fn deploy_app(host: &str, user: &str, dir: &str) -> Result<crate::api::JobView, String> {
    if !valid_user(user) || !crate::host::is_host_name(host) {
        return Err("not a person or a domain".into());
    }
    use std::os::unix::fs::PermissionsExt;
    let d = Path::new("/run/cove");
    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    let script = d.join("deploy-app.sh");
    std::fs::write(&script, DEPLOY_APP).map_err(|e| format!("{}: {e}", script.display()))?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let q = |s: &str| crate::api::shell_quote(s);
    let cmd = format!("bash {} {} {} {}", q(&script.display().to_string()), q(host), q(user), q(dir));
    crate::host::stack::jobs::start(deploy_kind(host), format!("deploy {host}"), cmd, None)
}

/// The script that starts a `kind` session; None for Cove's own `serve`.
fn launcher(ctx: &Ctx, kind: Kind) -> Option<&PathBuf> {
    match kind {
        Kind::Ide => None,
        Kind::RStudio => ctx.conf.rstudio_launcher.as_ref(),
        Kind::Positron => ctx.conf.positron_launcher.as_ref(),
    }
}

/// Which Claude `owner` has (host.rs claude_lookup), kept ten minutes so a
/// second session starts at once.
type ClaudeCache = Mutex<HashMap<String, (Instant, Option<(String, String, String)>)>>;

fn claude_cache() -> &'static ClaudeCache {
    static CACHE: std::sync::OnceLock<ClaudeCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn claude_for(owner: &str) -> Option<(String, String, String)> {
    let cache = claude_cache();
    if let Some((at, v)) = cache.lock().unwrap().get(owner) {
        if at.elapsed() < Duration::from_secs(600) {
            return v.clone();
        }
    }
    let v = crate::host::claude_lookup(owner);
    cache.lock().unwrap().insert(owner.to_owned(), (Instant::now(), v.clone()));
    v
}

/// "This machine's login" is the platform account's ~/.claude: true only for a
/// session that runs as that account.
fn claude_label_for(a: &Account, (k, v, l): (String, String, String)) -> (String, String, String) {
    if k == "own_login" && platform_uid() != Some(a.uid) {
        (k, v, "your own claude login - /login once in claude".to_owned())
    } else {
        (k, v, l)
    }
}

/// One account's ceiling. The platform account (the operator's run-as) is
/// placed too, so it is measured, but never limited.
fn limits_for(ctx: &Ctx, uid: u32) -> crate::host::limits::Limits {
    if platform_uid() == Some(uid) {
        crate::host::limits::Limits::NONE
    } else {
        ctx.conf.person
    }
}

fn platform_uid() -> Option<u32> {
    std::fs::metadata(crate::host::platform_root()).map(|m| std::os::unix::fs::MetadataExt::uid(&m)).ok()
}

/// The person's Claude where their sessions' terminals read it (host.rs
/// CLAUDE_DIR, `claude_env`): root-owned, their group may read, nobody else.
fn write_claude_file(owner: &str, gid: u32, (k, v, l): &(String, String, String)) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = Path::new(crate::host::CLAUDE_DIR);
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
    let tmp = dir.join(format!(".{owner}.json.tmp"));
    std::fs::write(&tmp, serde_json::json!({ "kind": k, "value": v, "label": l }).to_string())?;
    std::os::unix::fs::chown(&tmp, Some(0), Some(gid))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o640))?;
    std::fs::rename(&tmp, dir.join(format!("{owner}.json")))
}

/// Run ids that are each person's (`on_behalf`): started for them, or listed
/// in their own history - a Stop or a poll reaches only their own runs.
fn started_runs() -> &'static Mutex<HashMap<String, String>> {
    static RUNS: std::sync::OnceLock<Mutex<HashMap<String, String>>> = std::sync::OnceLock::new();
    RUNS.get_or_init(Default::default)
}

/// Whether `owner` may poll or stop `run`: one they started here, or - since
/// a restart empties `started_runs` while open sessions keep polling (2026-10-18:
/// ~7 "not one of yours" 500s a second after the 14:38 restart, and every
/// open agent turn froze) - a turn found in one of their threads on disk.
fn run_is_theirs(ctx: &Ctx, owner: &str, account: &Account, run: &str) -> bool {
    if started_runs().lock().unwrap().get(run).map(String::as_str) == Some(owner) {
        return true;
    }
    let ok = is_admin(ctx, owner)
        || crate::host::run_in_thread_as(owner, "exec.ndexr.io", run)
        || workable_domains(owner, account).iter().any(|(h, _)| crate::host::run_in_thread_as(owner, h, run));
    if ok {
        started_runs().lock().unwrap().insert(run.to_owned(), owner.to_owned());
    }
    ok
}

/// `owner` is an ndexr admin (host.rs ndexr_admin), asked at most every five
/// minutes: Builds asks for the machines every 30 s.
fn ndexr_admin_cached(owner: &str) -> bool {
    static SEEN: std::sync::OnceLock<Mutex<HashMap<String, (Instant, bool)>>> = std::sync::OnceLock::new();
    let seen = SEEN.get_or_init(Default::default);
    if let Some((at, a)) = seen.lock().unwrap().get(owner) {
        if at.elapsed() < Duration::from_secs(300) {
            return *a;
        }
    }
    let a = crate::host::ndexr_admin(owner);
    seen.lock().unwrap().insert(owner.to_owned(), (Instant::now(), a));
    a
}

/// A session's /api/q that only this (root) server can answer for the person:
/// their Claude (setup.rs - the store is the platform's), and, for a session
/// whose account cannot read the platform checkout, the agent's turns and Help ›
/// Send a request. Until 2026-10-15 those ran inside the session, so the
/// Agent tab worked for the operator alone - every other account got "this
/// needs the platform checked out on this machine". None: the session answers.
fn on_behalf(ctx: &Ctx, owner: &str, id: &str, body: &[u8]) -> Option<Resp> {
    let q: crate::api::Query = serde_json::from_slice(body).ok()?;
    let answer = answer_on_behalf(ctx, owner, id, &q)?;
    Some(match answer {
        Ok(v) => Response::from_data(v.to_string().into_bytes()).with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
        Err(e) => Response::from_data(e.into_bytes()).with_status_code(500),
    })
}

/// What only this server answers for a person (their session, `id`, cannot):
/// None leaves the query to the session. Cove's page and the MCP connector
/// (mcp.rs, through `McpHost`) both come through here.
fn answer_on_behalf(ctx: &Ctx, owner: &str, id: &str, q: &crate::api::Query) -> Option<Result<serde_json::Value, String>> {
    use crate::api::Query;
    let q = q.clone();
    let account = ctx.sessions.lock().unwrap().get(id).map(|s| s.account.clone())?;
    // A page that loads asks something first: by the time the Account tab is
    // opened its answer is here (after a restart the cache is empty).
    crate::host::account_warm_if_cold(owner);
    let platform_session = platform_uid() == Some(account.uid);
    let answer: Result<serde_json::Value, String> = match &q {
        Query::ClaudeStatus | Query::ClaudeConnect { .. } | Query::ClaudeForget => {
            let (op, token) = match &q {
                Query::ClaudeConnect { token } => ("connect", token.as_str()),
                Query::ClaudeForget => ("forget", ""),
                _ => ("status", ""),
            };
            let r = crate::host::claude_as(owner, op, token);
            if op != "status" && r.is_ok() {
                audit(ctx, owner, "claude", op);
                // The next terminal runs on what they just connected (or not).
                let fresh = crate::host::claude_lookup(owner).map(|c| claude_label_for(&account, c));
                claude_cache().lock().unwrap().insert(owner.to_owned(), (Instant::now(), fresh.clone()));
                if let Some(c) = &fresh {
                    let _ = write_claude_file(owner, account.gid, c);
                }
            }
            r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        // The Account tab: always the owner's own account, whatever the body
        // says - account.r keys every read and the revoke by their email.
        // Messages: the platform's database, which only this server reads.
        Query::Inbox { all, chat, find } => crate::host::messages::inbox(owner, *all, chat, find).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        Query::ProjectNames { names } => crate::host::messages::project_names(owner, names).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        Query::InboxAct { act } => {
            let r = crate::host::messages::act(owner, act);
            // A share, or a place in a group, may change who works on what.
            if r.is_ok() && matches!(act, crate::api::InboxAct::Share { .. } | crate::api::InboxAct::ShareDecide { .. } | crate::api::InboxAct::ShareRevoke { .. } | crate::api::InboxAct::GroupJoin { .. } | crate::api::InboxAct::GroupLeave { .. }) {
                if let Some(kick) = SHARES_KICK.get() {
                    let _ = kick.send(());
                }
            }
            r
        }
        // Packages: the kit is under /home/ubuntu (0750) and the sessions are
        // this server's, so it answers - anyone signed in, the stack being
        // public; how many sessions load each module, an admin only.
        Query::PackageFlow { set, package } => {
            let mut f = crate::host::pkgflow::flow(set, package);
            // The build machines now: builds.json, public like /view/builds.json.
            if let Ok(b) = crate::host::builds_live() {
                f.live_machines = crate::host::pkgflow::live_machines(&b);
            }
            if ndexr_admin_cached(owner) {
                let mut by: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
                for s in ctx.sessions.lock().unwrap().values() {
                    let m = if s.module.trim().is_empty() { "none".to_owned() } else { s.module.trim().to_owned() };
                    *by.entry(m).or_default() += 1;
                }
                f.sessions = by.into_iter().map(|(module, sessions)| crate::api::PfUse { module, sessions }).collect();
            }
            serde_json::to_value(f).map_err(|e| e.to_string())
        }
        // Go's Recap: every account's work, so an ndexr admin's only.
        Query::Recap { minutes } => {
            if ndexr_admin_cached(owner) {
                serde_json::to_value(crate::host::recap::recap(*minutes)).map_err(|e| e.to_string())
            } else {
                Err("the recap is for ndexr admins".into())
            }
        }
        // Speak: the platform's OpenAI key, which no person's process holds.
        Query::Spoken { asked, reply } => crate::host::brief::spoken(asked, reply).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        Query::ReadAloud { text } => crate::host::brief::read_aloud(text).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        Query::RecapBrief { minutes, fresh } => {
            if ndexr_admin_cached(owner) {
                serde_json::to_value(crate::host::brief::brief(*minutes, *fresh)).map_err(|e| e.to_string())
            } else {
                Err("the briefing is for ndexr admins".into())
            }
        }
        Query::DmFile { id } => crate::host::messages::dm_file(owner, *id).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        Query::Account => crate::host::account_as(owner, "").and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        Query::RevokeKey { key_id } => {
            let r = crate::host::account_as(owner, key_id);
            if r.is_ok() {
                audit(ctx, owner, "revoke-key", key_id);
            }
            r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        // /agent in a console: the OpenAI key is the platform's, which only
        // this (root) server can read - answered here for every session.
        Query::Assist { lang, ask, context } => crate::host::assist(*lang, ask, context).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        // The build machines: an ndexr admin only (their platform record,
        // asked as them), every session including the operator's - so a
        // session, and the desktop app, can never reach them (boxes.rs).
        Query::HostVitals => {
            if !ndexr_admin_cached(owner) {
                Err("the machine's vitals are for ndexr admins".into())
            } else {
                crate::host::vitals::read().and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
            }
        }
        Query::Boxes | Query::BoxAct { .. } | Query::BoxJob { .. } => {
            if !ndexr_admin_cached(owner) {
                Err("the build machines are for ndexr admins".into())
            } else {
                match &q {
                    Query::BoxAct { server, verb } => {
                        let r = crate::host::boxes::act(server, verb);
                        if r.is_ok() {
                            audit(ctx, owner, "box", &format!("{server} {verb}"));
                        }
                        r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
                    }
                    Query::BoxJob { server } => serde_json::to_value(crate::host::boxes::job(server)).map_err(|e| e.to_string()),
                    _ => serde_json::to_value(crate::host::boxes::list()).map_err(|e| e.to_string()),
                }
            }
        }
        // The platform account's session answers the rest itself - except
        // deploying and routing an app, which need root and which the session
        // refuses ("apps are routed from cove.ndexr.io"): the operator's Rust
        // and Julia projects never went live (2026-10-26, ggsdfgg.ndexr.io).
        _ if platform_session && !matches!(q, Query::DeployApp { .. } | Query::DeployJob { .. } | Query::AppRoute { .. } | Query::ProjectRelease { .. } | Query::AppAccess { .. } | Query::SitePassword { .. }) => return None,
        Query::AgentRun { domain, prompt, model, cwd, mode } => crate::host::agent_run_as(owner, domain, prompt, model, cwd.as_deref(), mode.as_deref()).map(|run| {
            started_runs().lock().unwrap().insert(run.clone(), owner.to_owned());
            serde_json::Value::String(run)
        }),
        Query::AgentStop { id: run } => {
            if !run_is_theirs(ctx, owner, &account, run) {
                Err("that run is not one of yours".into())
            } else {
                crate::host::agent_stop_run(run).map(serde_json::Value::Bool)
            }
        }
        Query::SendRequest { domain, what, trying, blocked, files } => crate::host::send_request_as(owner, domain, what, trying, *blocked, files).map(serde_json::Value::from),
        // Save: their owner folder, committed and pushed by the platform
        // account (the ndexr CLI and the gitlab key are its) - every version
        // branch of their software projects with it. Inside the session it
        // needed `ndexr` on their PATH, which no account but the operator's has.
        Query::PushAll { .. } => crate::host::push_all_as(owner).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        Query::PushJob => serde_json::to_value(crate::host::push_job_as(owner)).map_err(|e| e.to_string()),
        // A new software project's domain (and its paradigm parent) recorded
        // as theirs; one of theirs restored from its saved repository.
        Query::DomainClaim { hosts } => {
            let r = crate::host::claim_as(owner, hosts);
            if r.is_ok() {
                audit(ctx, owner, "claim", &hosts.join(" "));
            }
            r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        // A Rust app of theirs, live: deploy-app.sh as root, building and
        // serving it as them (never as the platform account), routed behind
        // the owner gate. The script is compiled in and written root-only -
        // never run from the repo, which the ndexr group can write.
        Query::DeployApp { host } => {
            let h = host.trim().to_ascii_lowercase();
            match person_domains(owner, &account).into_iter().find(|(x, _)| *x == h) {
                None => Err(format!("{h} is not one of your domains")),
                Some((_, path)) => {
                    let r = deploy_app(&h, &account.name, &path);
                    audit(ctx, owner, "deploy-app", &format!("{h} {}", if r.is_ok() { "started" } else { "refused" }));
                    r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
                }
            }
        }
        // Versions: read and restored as the platform account, which owns the
        // repositories the autosave service keeps; only the person's own.
        Query::Versions { host } | Query::VersionRestore { host, .. } | Query::GitChanges { host, .. } | Query::GitDiff { host, .. } | Query::GitStatus { host } | Query::GitPatch { host, .. } => {
            let h = host.trim().to_ascii_lowercase();
            match person_domains(owner, &account).into_iter().find(|(x, _)| *x == h) {
                None => Err(format!("{h} is not one of your domains")),
                // Run as the platform account, which cannot enter their home:
                // the same folder by its platform path.
                Some((_, path)) => match (&q, crate::host::platform_paths(owner, &path)) {
                    (Query::VersionRestore { sha, .. }, path) => {
                        let r = crate::host::version_restore_at(Path::new(&path), sha, owner);
                        audit(ctx, owner, "version-restore", &format!("{h} {sha} {}", if r.is_ok() { "done" } else { "refused" }));
                        r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
                    }
                    (Query::GitChanges { sha, .. }, path) => crate::host::git::changes_at(Path::new(&path), sha).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
                    (Query::GitDiff { sha, path: file, old, area, full, .. }, path) => crate::host::git::diff_at(Path::new(&path), sha, file, old, area, *full).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
                    (Query::GitStatus { .. }, path) => crate::host::git::status_at(Path::new(&path)).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
                    // Changes the person's files or index: on the record.
                    (Query::GitPatch { action, patch, .. }, path) => {
                        let r = crate::host::git::patch_at(Path::new(&path), action, patch);
                        audit(ctx, owner, "git-patch", &format!("{h} {action} {} bytes {}", patch.len(), if r.is_ok() { "done" } else { "refused" }));
                        r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
                    }
                    (_, path) => crate::host::versions_at(Path::new(&path)).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
                },
            }
        }
        // A site's requests, from the web server's log (host.rs
        // `site_requests`): root reads it - it is every site's, with visitors'
        // addresses - and hands back only the person's own site's lines.
        Query::SiteRequests { host } => {
            let h = host.trim().to_ascii_lowercase();
            match person_domains(owner, &account).into_iter().find(|(x, _)| *x == h) {
                None => Err(format!("{h} is not one of your domains")),
                Some(_) => crate::host::site_requests(&h).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
            }
        }
        // The project's container image (docs/container-images.md): ndexr
        // builds, publishes and hands out pull logins as the platform account,
        // for the person's own domains only - the same check as Versions.
        Query::Image { host, verb } => {
            let h = host.trim().to_ascii_lowercase();
            match person_domains(owner, &account).into_iter().find(|(x, _)| *x == h) {
                None => Err(format!("{h} is not one of your domains")),
                Some((_, path)) => {
                    let r = crate::host::image_at(owner, Path::new(&crate::host::platform_paths(owner, &path)), &h, verb);
                    if verb == "build" || verb == "pull" {
                        audit(ctx, owner, &format!("image-{verb}"), &format!("{h} {}", if r.is_ok() { "done" } else { "refused" }));
                    }
                    r
                }
            }
        }
        // Archive, for a person whose account cannot read the platform's
        // domains: their own domain only (or an admin), run as the platform
        // account, one per person at a time (archive_kind).
        Query::Archive { host } => {
            let h = host.trim().to_ascii_lowercase();
            if !person_domains(owner, &account).iter().any(|(x, _)| *x == h) && !ndexr_admin_cached(owner) {
                Err(format!("{h} is not one of your domains"))
            } else {
                let r = crate::host::archive_cmd(&h).and_then(|cmd| {
                    let q = crate::api::shell_quote(&cmd);
                    crate::host::stack::jobs::start(archive_kind(owner), format!("archive {h}"), format!("runuser -u ubuntu -- bash -c {q}"), None)
                });
                audit(ctx, owner, "archive", &format!("{h} {}", if r.is_ok() { "started" } else { "refused" }));
                r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
            }
        }
        Query::ArchiveJob => serde_json::to_value(crate::host::stack::jobs::view(archive_kind(owner))).map_err(|e| e.to_string()),
        // Delete: Archive has removed the folder, so ownership is the
        // registry's to say (the script checks it), not person_domains'.
        Query::ProjectRelease { host } => {
            let h = host.trim().to_ascii_lowercase();
            let r = release_project(&h, owner, ndexr_admin_cached(owner));
            audit(ctx, owner, "project-delete", &format!("{h} {}", match &r { Ok(_) => "released".to_owned(), Err(e) => e.lines().last().unwrap_or("refused").to_owned() }));
            r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        Query::AppRoute { host } => {
            let h = host.trim().to_ascii_lowercase();
            match person_domains(owner, &account).into_iter().find(|(x, _)| *x == h) {
                None => Err(format!("{h} is not one of your domains")),
                Some((_, path)) => {
                    let r = route_app(&h, &account.name, &path);
                    audit(ctx, owner, "app-route", &format!("{h} {}", if r.is_ok() { "routed" } else { "refused" }));
                    r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
                }
            }
        }
        // Who may open a domain: its owner, someone it is shared with to
        // manage (shares.rs role_of), or an admin reads or sets it; "admin"
        // only an admin may choose.
        Query::AppAccess { host, level } => {
            let h = host.trim().to_ascii_lowercase();
            let admin = ndexr_admin_cached(owner);
            let lvl = level.trim();
            let manages = crate::host::shares::role_of(Path::new(&account.home), &h).as_deref() == Some("manage");
            if !admin && !manages && !person_domains(owner, &account).iter().any(|(x, _)| *x == h) {
                Err(format!("{h} is not one of your domains"))
            } else if !lvl.is_empty() && !["public", "login", "owner"].contains(&lvl) && !(admin && lvl == "admin") {
                Err(format!("{lvl}: not a level you can choose"))
            } else if lvl.is_empty() {
                app_access_read(&h).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
            } else {
                let r = app_access_set(&h, lvl);
                audit(ctx, owner, "app-access", &format!("{h} {lvl} {}", if r.is_ok() { "set" } else { "refused" }));
                r.and_then(|said| app_access_read(&h).map(|mut v| { v.said = said; v })).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
            }
        }
        Query::SitePassword { host, password } => {
            let h = host.trim().to_ascii_lowercase();
            let admin = ndexr_admin_cached(owner);
            let manages = crate::host::shares::role_of(Path::new(&account.home), &h).as_deref() == Some("manage");
            if !admin && !manages && !person_domains(owner, &account).iter().any(|(x, _)| *x == h) {
                Err(format!("{h} is not one of your domains"))
            } else {
                let r = site_password(&h, password, owner);
                // Whether it was set or removed - never the password.
                audit(ctx, owner, "site-password", &format!("{h} {} {}", if password.is_empty() { "cleared" } else { "set" }, if r.is_ok() { "ok" } else { "refused" }));
                r.map(serde_json::Value::from)
            }
        }
        Query::AppViewer { host, on } => {
            let h = host.trim().to_ascii_lowercase();
            let admin = ndexr_admin_cached(owner);
            let manages = crate::host::shares::role_of(Path::new(&account.home), &h).as_deref() == Some("manage");
            if !admin && !manages && !person_domains(owner, &account).iter().any(|(x, _)| *x == h) {
                Err(format!("{h} is not one of your domains"))
            } else {
                let r = crate::host::app_viewer(&h, *on);
                if let Some(v) = on {
                    audit(ctx, owner, "app-viewer", &format!("{h} {} {}", if *v { "listed" } else { "unlisted" }, if r.is_ok() { "set" } else { "refused" }));
                }
                r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
            }
        }
        Query::DeployJob { host } => {
            let h = host.trim().to_ascii_lowercase();
            if person_domains(owner, &account).iter().any(|(x, _)| *x == h) {
                serde_json::to_value(crate::host::stack::jobs::view(deploy_kind(&h))).map_err(|e| e.to_string())
            } else {
                Err(format!("{h} is not one of your domains"))
            }
        }
        Query::DomainRestore { host } => crate::host::restore_as(owner, host).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        // Publishing runs the platform's provisioner as the platform account:
        // the person's own session cannot reach the registry, DNS or nginx.
        Query::PublishDomain { host } => {
            // The level the project asks for (.cove/app.env APP_ACCESS, or
            // app.env at an old-layout root); owner - the person and who they
            // share it with - when it asks for none (operator, 2026-11-17).
            let h = host.trim().to_ascii_lowercase();
            let access = person_domains(owner, &account)
                .into_iter()
                .find(|(x, _)| *x == h)
                .and_then(|(_, p)| {
                    [".cove/app.env", "app.env"].iter().find_map(|f| std::fs::read_to_string(Path::new(&p).join(f)).ok())
                })
                .and_then(|env| env.lines().find_map(|l| l.trim().strip_prefix("APP_ACCESS=").map(|v| v.trim().trim_matches('"').to_owned())))
                .filter(|a| ["public", "login", "owner"].contains(&a.as_str()))
                .unwrap_or_else(|| "owner".to_owned());
            let r = crate::host::publish_with(owner, host, &access);
            audit(ctx, owner, "publish", &format!("{host} {}", if r.is_ok() { "started" } else { "refused" }));
            r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string()))
        }
        // The runs folder is the platform account's alone (no one else may read
        // another person's prompts), so a person's history and streams come
        // through here - only their own: a domain of theirs, their Exec thread,
        // and runs started for them or listed in their history.
        Query::AgentBusy { domains } => {
            let admin = is_admin(ctx, owner);
            let mine = workable_domains(owner, &account);
            let asked: Vec<String> = domains
                .iter()
                .map(|d| d.trim().to_ascii_lowercase())
                .filter(|d| admin || mine.iter().any(|(h, _)| h == d))
                .collect();
            serde_json::to_value(crate::host::agent_busy(owner, &asked)).map_err(|e| e.to_string())
        }
        Query::AgentHistory { domain } => {
            let d = domain.trim().to_ascii_lowercase();
            let theirs = d == "exec.ndexr.io" || is_admin(ctx, owner) || workable_domains(owner, &account).iter().any(|(h, _)| *h == d);
            if !theirs {
                Err(format!("{d} is not one of your domains"))
            } else {
                crate::host::agent_history_as(owner, &d).map(|turns| {
                    started_runs().lock().unwrap().extend(turns.iter().map(|t| (t.id.clone(), owner.to_owned())));
                    serde_json::to_value(turns).unwrap_or_default()
                })
            }
        }
        Query::AgentPoll { id: run, offset } => {
            if !run_is_theirs(ctx, owner, &account, run) {
                Err("that run is not one of yours".into())
            } else {
                crate::host::agent_poll(run, *offset).and_then(|c| serde_json::to_value(c).map_err(|e| e.to_string()))
            }
        }
        // The person's Claude connectors (mcp.rs), kept by this server.
        Query::McpGrants => Ok(crate::server::mcp::grants_of(owner)),
        Query::McpRevoke { id: grant } => {
            let r = crate::server::mcp::revoke(owner, grant);
            if r.is_ok() {
                audit(ctx, owner, "mcp-revoke", grant);
            }
            r
        }
        _ => return None,
    };
    Some(answer)
}

/// The MCP connector's way into Cove (mcp.rs `Host`): the person's projects,
/// and each query answered as them - by this server where only it can,
/// else by their latest Cove session (started if they have none), exactly
/// as Cove's page is answered.
struct McpHost<'a>(&'a Ctx);

impl crate::server::mcp::Host for McpHost<'_> {
    fn projects(&self, user: &str) -> Vec<(String, String)> {
        account_for(self.0, user).map(|a| workable_domains(user, &a)).unwrap_or_default()
    }

    fn ask(&self, user: &str, q: crate::api::Query) -> Result<serde_json::Value, String> {
        let ctx = self.0;
        let id = latest_or_new(ctx, user)?;
        if let Some(a) = answer_on_behalf(ctx, user, &id, &q) {
            return a;
        }
        let (port, token) = ctx.sessions.lock().unwrap().get(&id).filter(|s| s.owner == user).map(|s| (s.port, s.token.clone())).ok_or("your Cove session ended - try again")?;
        let body = serde_json::to_vec(&q).map_err(|e| e.to_string())?;
        session_post(port, &token, &body)
    }

    fn audit(&self, user: &str, what: &str, detail: &str) {
        audit(self.0, user, what, detail);
    }
}

/// POST a query to a person's session at `port` and read its answer: 200 is
/// JSON, anything else its text as the error.
fn session_post(port: u16, token: &str, body: &[u8]) -> Result<serde_json::Value, String> {
    let until = Instant::now() + Duration::from_secs(20);
    let mut s = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("your Cove session is not answering: {e}")),
        }
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(660)));
    let head = format!("POST /api/q HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: cove={token}\r\nX-Cove: 1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    s.write_all(head.as_bytes()).and_then(|_| s.write_all(body)).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no answer from your session")?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut body = raw[split + 4..].to_vec();
    if head.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        body = unchunk(&body);
    }
    let status = head.split_whitespace().nth(1).and_then(|c| c.parse::<u16>().ok()).unwrap_or(0);
    if status == 200 {
        serde_json::from_slice(&body).map_err(|e| format!("an answer that is not JSON: {e}"))
    } else {
        Err(String::from_utf8_lossy(&body).trim().to_owned())
    }
}

fn unchunk(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(eol) = b[i..].windows(2).position(|w| w == b"\r\n") {
        let n = usize::from_str_radix(String::from_utf8_lossy(&b[i..i + eol]).trim(), 16).unwrap_or(0);
        i += eol + 2;
        if n == 0 || i + n > b.len() {
            break;
        }
        out.extend_from_slice(&b[i..i + n]);
        i += n + 2;
    }
    out
}

/// Start a new session for `owner`: an `cove serve` as their account,
/// with its own settings and cache folders and the module and project asked
/// for, on a free loopback port.
fn new_session(ctx: &Ctx, owner: &str, name: &str, module: &str, project: &str, kind: Kind) -> Result<String, String> {
    start_session(ctx, owner, name, module, project, kind, None)
}

/// Start a session; `again` is (id, port, token) of one being started anew on
/// the current build (`update_session`), so its page, address and settings
/// folders stay the same.
fn start_session(ctx: &Ctx, owner: &str, name: &str, module: &str, project: &str, kind: Kind, again: Option<(String, u16, String)>) -> Result<String, String> {
    if kind.direct() && launcher(ctx, kind).is_none() {
        return Err(format!("{} sessions are not set up on this server ({}-launcher)", kind.label(), kind.key()));
    }
    if !module.is_empty() && !crate::host::kernel::valid_module(module) {
        return Err(format!("not a module name: {module}"));
    }
    if !project.is_empty() && (!project.starts_with('/') || project.contains("/../") || project.ends_with("/..")) {
        return Err("the project folder is an absolute path".into());
    }
    let a = account_for(ctx, owner)?;
    // Their Account tab's answer, asked while the session starts.
    crate::host::account_warm_if_cold(owner);
    // The person's own Claude, for `claude` in the session's terminal - looked
    // up here, as root: their account cannot read the platform it lives in.
    let claude = if kind.direct() { None } else { claude_for(owner) }.map(|c| claude_label_for(&a, c));
    if let Some(c) = &claude {
        let _ = write_claude_file(owner, a.gid, c);
    }
    let mut all = ctx.sessions.lock().unwrap();
    all.retain(|_, s| alive(s));
    let mine = all.values().filter(|s| s.owner == owner).count();
    if again.is_none() && mine >= ctx.conf.max_per_user {
        return Err(format!("you have {mine} sessions, the most this server allows - quit one first"));
    }
    if again.is_none() && all.len() >= ctx.conf.max_total {
        return Err("this server is running as many sessions as it allows - try again later".into());
    }
    let used: Vec<u16> = all.values().map(|s| s.port).collect();
    let (id, port, token) = match again {
        Some(t) => t,
        None => {
            let port = (ctx.conf.ports.0..=ctx.conf.ports.1).find(|p| !used.contains(p) && port_free(*p)).ok_or("no free session port")?;
            (random_hex(5), port, random_hex(24))
        }
    };
    let (release, exe, web) = current_release(ctx);
    let name = {
        let n: String = name.trim().chars().filter(|c| !c.is_control()).take(60).collect();
        if n.is_empty() {
            format!("Session {}", mine + 1)
        } else {
            n
        }
    };
    let seed = serde_json::to_string(&crate::api::Settings { module: module.to_owned(), project_dir: project.to_owned(), ..Default::default() }).unwrap_or_default();
    let home = if Path::new(&a.home).is_dir() { a.home.clone() } else { "/".into() };
    // The account's group: every session of theirs, and all it starts, shares
    // one ceiling.
    let limits = limits_for(ctx, a.uid);
    let enter = match &ctx.people {
        Some(people) => Some(CString::new(crate::host::limits::place(people, a.uid, limits)?.into_os_string().into_encoded_bytes()).map_err(|_| "bad cgroup path")?),
        None => None,
    };
    let mut cmd = match launcher(ctx, kind) {
        Some(launcher) => {
            let mut c = Command::new(launcher);
            c.arg(port.to_string()).arg(&id).arg(if project.is_empty() { home.as_str() } else { project }).arg(module);
            c
        }
        _ => {
            let mut c = Command::new(&exe);
            c.args(["serve", "--listen", &format!("127.0.0.1:{port}"), "--web-dir"]).arg(&web);
            c
        }
    };
    cmd.env_clear()
        .env("HOME", &a.home)
        .env("USER", &a.name)
        .env("LOGNAME", &a.name)
        .env("SHELL", if a.shell.is_empty() { "/bin/bash" } else { &a.shell })
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("LANG", std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".into()))
        // Each session its own consoles' settings, plots and drivers.
        .env("XDG_CONFIG_HOME", format!("{home}/.config/cove/sessions/{id}"))
        .env("XDG_CACHE_HOME", format!("{home}/.cache/cove/sessions/{id}"))
        // Who signed in - the ndexr username the session's agent runs as
        // (host.rs agent_run), whatever account the session runs under.
        .env("COVE_OWNER", owner)
        .env("COVE_SEED", seed)
        .env("COVE_TOKEN", &token)
        .env("COVE_UPDATE_SECS", "0")
        .env("COVE_RELEASE", &release)
        .envs(claude.iter().flat_map(|(k, v, l)| [("COVE_CLAUDE_KIND", k.clone()), ("COVE_CLAUDE_VALUE", v.clone()), ("COVE_CLAUDE_LABEL", l.clone())]))
        .envs(crate::host::limits::job_env(limits))
        .current_dir(&home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    {
        use std::os::unix::process::CommandExt;
        let cname = CString::new(a.name.clone()).map_err(|_| "bad name")?;
        let (uid, gid) = (a.uid, a.gid);
        cmd.process_group(0);
        // Groups, then gid, then uid - in that order, while still root. Done
        // here rather than with Command::uid, which drops supplementary groups.
        unsafe {
            cmd.pre_exec(move || {
                // Into the account's group while still root (a person cannot
                // move themselves); "0" is this process.
                if let Some(procs) = &enter {
                    let fd = libc::open(procs.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
                    if fd < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    let wrote = libc::write(fd, b"0".as_ptr().cast(), 1) == 1;
                    let err = std::io::Error::last_os_error();
                    libc::close(fd);
                    if !wrote {
                        return Err(err);
                    }
                }
                if libc::initgroups(cname.as_ptr(), gid as _) != 0 || libc::setgid(gid) != 0 || libc::setuid(uid) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getuid() == 0 || libc::geteuid() == 0 {
                    return Err(std::io::Error::other("still root"));
                }
                Ok(())
            });
        }
    }
    let child = cmd.spawn().map_err(|e| format!("could not start a session: {e}"))?;
    all.insert(
        id.clone(),
        Session { id: id.clone(), kind, owner: owner.to_owned(), account: a.clone(), name: name.clone(), module: module.to_owned(), project: project.to_owned(), port, token, child: Proc::Child(child), release: release.clone(), started: SystemTime::now(), last: Instant::now() },
    );
    drop(all);
    save_sessions(ctx);
    audit(ctx, owner, "session-start", &format!("{id} {} \"{name}\" as {} module={} project={} port={port} release={}", kind.label(), a.name, if module.is_empty() { "-" } else { module }, if project.is_empty() { "-" } else { project }, if release.is_empty() { "-" } else { &release }));
    // Ready when it answers. An rserver loads the stack's modules first.
    let until = Instant::now() + Duration::from_secs(if kind.direct() { 60 } else { 15 });
    while Instant::now() < until {
        if !port_free(port) {
            return Ok(id);
        }
        // A launcher that gave up (no rserver on the stack, no R) is not
        // waited out for a minute.
        if ctx.sessions.lock().unwrap().get_mut(&id).is_some_and(|s| !alive(s)) {
            stop(ctx, &id, owner, "ended while starting");
            return Err(format!("the {} session ended while starting - the server's log says why", kind.label()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    stop(ctx, &id, owner, "did not start");
    Err("the session did not start".into())
}

/// End a session: SIGTERM, which `serve` turns into ending its kernels, then
/// SIGKILL to the group if it has not gone in 5 s.
fn stop(ctx: &Ctx, id: &str, by: &str, why: &str) -> bool {
    let Some(mut s) = ctx.sessions.lock().unwrap().remove(id) else { return false };
    save_sessions(ctx);
    end_proc(&mut s.child);
    audit(ctx, by, "session-stop", &format!("{} \"{}\" of {} ({why})", s.id, s.name, s.owner));
    true
}

fn end_proc(p: &mut Proc) {
    let pid = p.pid();
    unsafe { libc::kill(pid, libc::SIGTERM) };
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if !p.running() {
            p.reap();
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    unsafe { libc::kill(-pid, libc::SIGKILL) };
    p.reap();
}

/// The person asked for the newer Cove: this session ends and starts again on
/// the current build - same id, address and token, so the page reloads into
/// it; same settings folders, so its environment, project and layout stay.
/// R, Python and the terminal start fresh; agents run in scopes of their own
/// and carry on.
fn update_session(ctx: &Ctx, user: &str, id: &str) -> Result<String, String> {
    let s = ctx.sessions.lock().unwrap().remove(id).ok_or("no such session")?;
    if s.owner != user || s.kind != Kind::Ide {
        let ok = s.owner == user;
        ctx.sessions.lock().unwrap().insert(id.to_owned(), s);
        return Err(if ok { "only Cove sessions update this way".into() } else { "not your session".into() });
    }
    let mut s = s;
    let was = s.release.clone();
    end_proc(&mut s.child);
    save_sessions(ctx);
    let r = start_session(ctx, &s.owner, &s.name, &s.module, &s.project, s.kind, Some((s.id.clone(), s.port, s.token.clone())));
    audit(ctx, user, "session-update", &format!("{id} from release {} ({})", if was.is_empty() { "-" } else { &was }, match &r { Ok(_) => "ok".to_owned(), Err(e) => e.clone() }));
    r
}

/// Every app port in use on the machine, for the sessions' free_app_port
/// (host.rs): this process is root and sees every project and vhost, a
/// session sees only its own person's. Written whole, then renamed.
fn publish_taken_ports() {
    let mut ports: Vec<u16> = crate::host::taken_ports().into_iter().collect();
    ports.sort_unstable();
    let body: String = ports.iter().map(|p| format!("{p}\n")).collect();
    let to = crate::host::ports_file();
    let tmp = to.with_extension("new");
    if std::fs::write(&tmp, body).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o644));
        }
        let _ = std::fs::rename(&tmp, &to);
    }
}

fn reaper(ctx: Arc<Ctx>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let idle: Vec<String> = ctx.sessions.lock().unwrap().iter_mut().filter_map(|(id, s)| (s.last.elapsed() > ctx.conf.idle || !alive(s)).then(|| id.clone())).collect();
        for id in idle {
            stop(&ctx, &id, "server", &format!("idle over {} min, or ended", ctx.conf.idle.as_secs() / 60));
        }
        ctx.cookies.lock().unwrap().retain(|_, c| c.expires > Instant::now());
        publish_taken_ports();
    });
}

// ── HTTP ────────────────────────────────────────────────────────────────────

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

fn html(status: u16, body: String) -> Resp {
    Response::from_data(body.into_bytes())
        .with_status_code(status)
        .with_header(header("Content-Type", "text/html; charset=utf-8"))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'"))
}

fn redirect(to: &str) -> Resp {
    Response::from_data(Vec::new()).with_status_code(302).with_header(header("Location", to))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn get_header<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn cookie(req: &Request) -> Option<String> {
    cookie_named(req, COOKIE)
}

fn cookie_named(req: &Request, name: &str) -> Option<String> {
    req.headers().iter().filter(|h| h.field.equiv("Cookie")).flat_map(|h| h.value.as_str().split(';')).find_map(|c| {
        let (k, v) = c.trim().split_once('=')?;
        (k == name).then(|| v.to_owned())
    })
}

/// A direct session's capability cookie: scoped to its own /r/<id>/ or
/// /p/<id>/, so a person's several sessions never see each other's.
fn direct_cookie(kind: Kind, id: &str, token: &str) -> Header {
    header("Set-Cookie", &format!("ndexr_rs_{id}={token}; Path={}; HttpOnly; Secure; SameSite=Lax; Max-Age=43200", kind.url(id)))
}

/// Into an RStudio or Positron session: there, with its cookie. Positron's
/// own front page also takes its connection token once (?tkn=, which it trades
/// for its own cookie) and the folder to open.
fn enter_direct(kind: Kind, id: &str, token: &str, folder: &str, rest: &str) -> Resp {
    let to = if kind == Kind::Positron && (rest.is_empty() || rest.starts_with('?')) {
        let more = rest.strip_prefix('?').map(|q| format!("&{q}")).unwrap_or_default();
        let folder = if folder.is_empty() || more.contains("folder=") { String::new() } else { format!("&folder={}", encode(folder)) };
        format!("{}?tkn={token}{folder}{more}", kind.url(id))
    } else {
        format!("{}{rest}", kind.url(id))
    };
    redirect(&to).with_header(direct_cookie(kind, id, token))
}

/// A direct session of `user`'s, entered: its token and folder, if it is alive.
fn enter_session(ctx: &Ctx, user: &str, id: &str, rest: &str) -> Option<Resp> {
    let mut all = ctx.sessions.lock().unwrap();
    let s = all.get_mut(id).filter(|s| s.owner == user && s.kind.direct())?;
    alive(s).then(|| enter_direct(s.kind, id, &s.token, &s.project, rest))
}

/// console.ndexr.io's sign-in, coming back to `url` on the host the person
/// asked for - an ndexr.io host only, so the link cannot send anyone
/// elsewhere.
fn sign_in_url(req: &Request, url: &str) -> String {
    let host = get_header(req, "X-Forwarded-Host").or_else(|| get_header(req, "Host")).unwrap_or("");
    let host = if host == "ndexr.io" || host.ends_with(".ndexr.io") { host } else { "cove.ndexr.io" };
    let back = format!("https://{host}{}", if url.starts_with('/') { url } else { "/" });
    let enc: String = back.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~:/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
    format!("https://console.ndexr.io/?next={enc}")
}

/// The editor a bare request means: Inlet on an inlet.* host, Positron on a
/// positron.* host, else the ide. rstudio.ndexr.io was retired 2026-10-29
/// with no redirect (Posit's marks stay out of hostnames), so it is not here.
fn host_kind(req: &Request) -> Kind {
    kind_of_host(get_header(req, "X-Forwarded-Host").or_else(|| get_header(req, "Host")).unwrap_or(""))
}

fn kind_of_host(host: &str) -> Kind {
    if host.starts_with("inlet.") {
        Kind::RStudio
    } else if host.starts_with("positron.") {
        Kind::Positron
    } else {
        Kind::Ide
    }
}

fn form(body: &[u8]) -> HashMap<String, String> {
    String::from_utf8_lossy(body)
        .split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((decode(k)?, decode(v)?))
        })
        .collect()
}

fn query(url: &str) -> HashMap<String, String> {
    url.split_once('?').map(|(_, q)| form(q.as_bytes())).unwrap_or_default()
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                out.push(u8::from_str_radix(std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A form post from another site is refused: SameSite already keeps the
/// cookie off it, and the Origin, when the browser sends one, must be ours.
fn same_origin(req: &Request) -> bool {
    let host = get_header(req, "X-Forwarded-Host").or_else(|| get_header(req, "Host"));
    match (get_header(req, "Origin"), host) {
        (Some(o), Some(h)) => o.split("://").nth(1).is_some_and(|rest| rest == h),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// Who is asking. Proxy: the header, only with the proxy's key. PAM: our cookie.
fn caller(ctx: &Ctx, req: &Request) -> Option<String> {
    match &ctx.conf.auth {
        Auth::Proxy { header, key } => {
            let k = get_header(req, "X-Cove-Key")?;
            if !same(k.trim(), key) {
                return None;
            }
            let u = get_header(req, header)?.trim().to_lowercase();
            valid_user(&u).then_some(u)
        }
        Auth::Pam => {
            let sid = cookie(req)?;
            let c = ctx.cookies.lock().unwrap();
            c.get(&sid).filter(|s| s.expires > Instant::now()).map(|s| s.user.clone())
        }
    }
}

fn is_admin(ctx: &Ctx, user: &str) -> bool {
    ctx.conf.admin_users.iter().any(|u| u == user) || account(user).is_some_and(|a| in_any(&a, &ctx.conf.admin_groups))
}

const STYLE: &str = "body{margin:0;min-height:100vh;background:#15171c;color:#d5dae3;font:14px/1.45 system-ui,sans-serif}main{max-width:980px;margin:0 auto;padding:28px 16px}form.box{background:#1d2027;border:1px solid #2b2f3d;padding:28px;width:min(92vw,380px);margin:12vh auto}input,select{width:100%;box-sizing:border-box;padding:9px;margin:4px 0 12px;border:1px solid #2b2f3d;background:#15171c;color:inherit;font:inherit}button,a.btn{display:inline-block;padding:8px 14px;border:0;background:#5ec4c0;color:#15171c;font-weight:600;cursor:pointer;text-decoration:none}button.stop{background:#e5767a}.card{background:#1d2027;border:1px solid #2b2f3d;padding:18px;margin:14px 0;overflow-x:auto}table{width:100%;border-collapse:collapse}td,th{text-align:left;padding:8px;border-bottom:1px solid #2b2f3d;vertical-align:middle}th{color:#8b93a2;font-weight:500}.err{color:#e8a75d}.mute{color:#8b93a2}a{color:#5ec4c0}code{font-family:ui-monospace,monospace;font-size:12.5px}form.inline{display:inline;margin:0}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(220px,1fr));gap:0 14px}pre{white-space:pre-wrap;font:12px ui-monospace,monospace;color:#8b93a2;margin:0}@media(max-width:640px){td,th{padding:6px 4px}.hide-sm{display:none}}";

fn page(title: &str, body: String) -> String {
    format!("<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>{} · Cove</title><style>{STYLE}</style>{body}", esc(title))
}

fn login_page(msg: &str) -> Resp {
    let err = if msg.is_empty() { String::new() } else { format!("<p class=err>{}</p>", esc(msg)) };
    html(
        if msg.is_empty() { 200 } else { 401 },
        page("sign in", format!("<form class=box method=post action=/login><b>Cove</b><p class=mute>Sign in with your account on this machine.</p>{err}<label>Username<input name=user autocomplete=username autofocus required></label><label>Password<input name=pass type=password autocomplete=current-password required></label><button>Sign in</button></form>")),
    )
}

fn sign_in(ctx: &Ctx, req: &mut Request) -> Resp {
    let mut body = Vec::new();
    let _ = req.as_reader().take(64 << 10).read_to_end(&mut body);
    let f = form(&body);
    let user = f.get("user").map(|u| u.trim().to_lowercase()).unwrap_or_default();
    let pass = f.get("pass").cloned().unwrap_or_default();
    if !valid_user(&user) || pass.is_empty() {
        return login_page("Enter your username and password.");
    }
    // Ten failures in ten minutes and the name waits out the window.
    {
        let mut fails = ctx.failures.lock().unwrap();
        let e = fails.entry(user.clone()).or_insert((0, Instant::now()));
        if e.1.elapsed() > Duration::from_secs(600) {
            *e = (0, Instant::now());
        }
        if e.0 >= 10 {
            return login_page("Too many failed sign-ins for this name - wait ten minutes.");
        }
    }
    let ok = pam::check(pam_service(), &user, &pass).and_then(|_| account(&user).ok_or_else(|| "no such account".into())).and_then(|a| may_sign_in(&a, &ctx.conf));
    match ok {
        Ok(()) => {
            ctx.failures.lock().unwrap().remove(&user);
            let sid = random_hex(32);
            ctx.cookies.lock().unwrap().insert(sid.clone(), Signin { user: user.clone(), expires: Instant::now() + Duration::from_secs(12 * 3600) });
            audit(ctx, &user, "sign-in", if is_admin(ctx, &user) { "admin" } else { "" });
            let secure = get_header(req, "X-Forwarded-Proto") == Some("https");
            redirect("/").with_header(header("Set-Cookie", &format!("{COOKIE}={sid}; Path=/; HttpOnly; SameSite=Strict; Max-Age=43200{}", if secure { "; Secure" } else { "" })))
        }
        Err(e) => {
            ctx.failures.lock().unwrap().entry(user.clone()).or_insert((0, Instant::now())).0 += 1;
            audit(ctx, &user, "sign-in-refused", &e);
            // A wrong password and an unknown name read the same from outside.
            std::thread::sleep(Duration::from_millis(800));
            login_page(if e.contains("group") || e.contains("system accounts") || e.contains("login shell") { "This account may not sign in here." } else { "Sign-in failed." })
        }
    }
}

fn ago(secs: u64) -> String {
    match secs {
        s if s < 90 => "just now".into(),
        s if s < 5400 => format!("{} min", s / 60),
        s => format!("{:.1} h", s as f64 / 3600.0),
    }
}

fn module_options(m: &crate::api::Modules, pick: &str) -> String {
    let opt = |v: &str| format!("<option value='{}'{}>{}</option>", esc(v), if v == pick { " selected" } else { "" }, esc(v));
    let mut o = String::new();
    for (title, names) in [("Environments · R + Python + site library", &m.env), ("R only", &m.r), ("Python only", &m.python)] {
        if !names.is_empty() {
            o.push_str(&format!("<optgroup label='{}'>{}</optgroup>", esc(title), names.iter().map(|n| opt(n)).collect::<String>()));
        }
    }
    o.push_str(&format!("<option value=''{}>none - R and python3 on PATH</option>", if pick.is_empty() { " selected" } else { "" }));
    o
}

/// One row of a session list: (owner, id, name, run-as, module, project, port, up, idle, kind).
type Row = (String, String, String, String, String, String, u16, u64, u64, Kind);

fn rows(ctx: &Ctx, owner: Option<&str>) -> Vec<Row> {
    let mut v: Vec<Row> = ctx
        .sessions
        .lock()
        .unwrap()
        .values_mut()
        .filter(|s| owner.is_none_or(|o| s.owner == o))
        .filter_map(|s| alive(s).then(|| (s.owner.clone(), s.id.clone(), s.name.clone(), s.account.name.clone(), s.module.clone(), s.project.clone(), s.port, s.started.elapsed().map(|d| d.as_secs()).unwrap_or(0), s.last.elapsed().as_secs(), s.kind)))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0).then(a.8.cmp(&b.8)));
    v
}

/// What a Cove host shows when it cannot take the person into a session.
/// There is no list to fall back on (2026-10-18): the sessions are Cove's
/// Recent menu, from inside one.
fn home_page(_ctx: &Ctx, _user: &str, note: &str) -> Resp {
    html(503, page("cove", format!("<main><p><b>Cove</b></p><p class=err>{}</p><p><a class=btn href=/>Try again</a></p></main>", esc(note))))
}

/// The front of an rstudio.* host: the person's RStudio sessions, and a new
/// one. `prefer` is the host's kind; Cove's own sessions are not listed here.
fn home_page_for(ctx: &Ctx, user: &str, note: &str, prefer: Kind) -> Resp {
    if !prefer.direct() {
        return home_page(ctx, user, note);
    }
    let mine: Vec<Row> = rows(ctx, Some(user)).into_iter().filter(|r| r.9 == prefer).collect();
    let mut t = String::new();
    for (_, id, name, _, module, project, _, up, idle, kind) in &mine {
        let href = kind.url(id);
        t.push_str(&format!(
            "<tr><td><a href='{href}'><b>{name}</b></a></td><td><code>{module}</code></td><td class=hide-sm><code>{project}</code></td><td class=hide-sm>up {up} · active {idle}</td><td style='white-space:nowrap'><a class=btn href='{href}'>Open</a> <form class=inline method=post action=/quit><input type=hidden name=id value='{id}'><button class=stop>Quit</button></form></td></tr>",
            name = esc(name),
            module = if module.is_empty() { "on PATH".into() } else { esc(module) },
            project = if project.is_empty() { "home".into() } else { esc(project) },
            up = ago(*up),
            idle = ago(*idle),
        ));
    }
    if mine.is_empty() {
        t.push_str("<tr><td colspan=5 class=mute>No sessions yet - start one below. Each session has its own R, Python and shell, its own environment and project.</td></tr>");
    }
    let m = modules(ctx);
    let last = mine.first().map(|s| s.4.clone()).unwrap_or_else(|| default_module(ctx));
    let doms = account_for(ctx, user).map(|a| workable_domains(user, &a)).unwrap_or_default();
    let places = if doms.is_empty() {
        "<option value=''>home - you have no domains yet</option>".to_owned()
    } else {
        doms.iter().map(|(h, p)| format!("<option value='{}'>{}</option>", esc(p), esc(h))).collect()
    };
    let note = if note.is_empty() { String::new() } else { format!("<p class=err>{}</p>", esc(note)) };
    let admin = if is_admin(ctx, user) { " · <a href=/admin>admin</a>" } else { "" };
    let out = if matches!(ctx.conf.auth, Auth::Pam) { " · <a href=/logout>sign out</a>" } else { "" };
    html(
        200,
        page(
            "sessions",
            format!(
                "<main><p class=mute><b>{label}</b> · {user}{admin}{out}</p>{note}<div class=card><h3 style='margin-top:0'>Your {label} sessions</h3><table><tr><th>Name</th><th>Environment</th><th class=hide-sm>Project</th><th class=hide-sm>Time</th><th></th></tr>{t}</table><p class=mute>{n} of {max} sessions. A session idle for {idle} min ends by itself.</p></div><div class=card><h3 style='margin-top:0'>New session</h3><form method=post action=/new><div class=grid><label>Name<input name=name placeholder='Session {next}' maxlength=60></label><label>Environment<select name=module>{opts}</select></label><label>Domain<select name=project>{places}</select></label></div><button>Start session</button></form></div></main>",
                user = esc(user),
                n = mine.len(),
                max = ctx.conf.max_per_user,
                idle = ctx.conf.idle.as_secs() / 60,
                next = mine.len() + 1,
                opts = module_options(&m, &last),
                places = places,
                label = prefer.label(),
            ),
        ),
    )
}

fn admin_page(ctx: &Ctx, me: &str, note: &str) -> Resp {
    let all = rows(ctx, None);
    let mut t = String::new();
    for (owner, id, name, as_, module, _, port, up, idle, kind) in &all {
        t.push_str(&format!(
            "<tr><td><b>{owner}</b>{as_}</td><td>{name} <span class=mute>{editor} · {id}</span></td><td><code>{module}</code></td><td class=hide-sm><code>:{port}</code></td><td class=hide-sm>up {up} · active {idle}</td><td><form class=inline method=post action=/admin><input type=hidden name=stop value='{id}'><button class=stop>Stop</button></form></td></tr>",
            owner = esc(owner),
            editor = kind.label(),
            as_ = if as_ != owner { format!(" <span class=mute>as {}</span>", esc(as_)) } else { String::new() },
            name = esc(name),
            module = if module.is_empty() { "on PATH".into() } else { esc(module) },
            up = ago(*up),
            idle = ago(*idle),
        ));
    }
    if all.is_empty() {
        t.push_str("<tr><td colspan=6 class=mute>No sessions running.</td></tr>");
    }
    let mut people: Vec<String> = all.iter().map(|s| s.0.clone()).collect();
    people.dedup();
    let stop_user: String = people
        .iter()
        .map(|p| format!("<form class=inline method=post action=/admin><input type=hidden name=stop_user value='{p}'><button class=stop>Stop all of {p}</button></form> ", p = esc(p)))
        .collect();
    let log = audit_tail(ctx, 60).iter().map(|l| esc(l)).collect::<Vec<_>>().join("\n");
    let mut act = String::new();
    for (p, at) in cove_people(ctx).iter().filter(|(p, _)| p != me) {
        act.push_str(&format!(
            "<tr><td><b>{p}</b></td><td class=mute>last opened {at}</td><td><form class=inline method=post action=/admin/as><input type=hidden name=user value='{p}'><button>Open Cove as {p}</button></form></td></tr>",
            p = esc(p),
            at = esc(at.get(..10).unwrap_or(at)),
        ));
    }
    if act.is_empty() {
        act.push_str("<tr><td colspan=3 class=mute>No one else has opened Cove yet.</td></tr>");
    }
    let c = &ctx.conf;
    let note = if note.is_empty() { String::new() } else { format!("<p class=err>{}</p>", esc(note)) };
    let auth = match &c.auth {
        Auth::Pam => format!("PAM ({})", pam_service()),
        Auth::Proxy { header, .. } => format!("a front proxy, naming the person in {} with its key", esc(header)),
    };
    let admins = [c.admin_users.join(", "), c.admin_groups.iter().map(|g| format!("@{g}")).collect::<Vec<_>>().join(", ")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", ");
    let run_as = if c.run_as.is_empty() { "everyone as themselves".to_owned() } else { c.run_as.iter().map(|(a, b)| format!("{a} as {b}")).collect::<Vec<_>>().join(", ") };
    html(
        200,
        page(
            "admin",
            format!(
                "<main><p class=mute><a href=/>← back</a> · admin · {me}</p><h2>Sessions</h2>{note}<div class=card><table><tr><th>Person</th><th>Session</th><th>Environment</th><th class=hide-sm>Port</th><th class=hide-sm>Time</th><th></th></tr>{t}</table><p>{stop_user}</p><p class=mute>{n} sessions, {p} people. Stop ends a session's R, Python and shell; the person can start another.</p></div><h2>Act as a person</h2><div class=card><table>{act}</table><p class=mute>Opens a new Cove session of theirs, as their account and on their own domain: what they see, and anything you do is done as them. An orange tag sits on the page while you act; <a href=/admin/as/stop>stop</a> ends it. Both are in the audit log.</p></div><h2>Policy</h2><div class=card><table><tr><td>Sign-in</td><td>{auth}</td></tr><tr><td>May sign in</td><td>{allow}</td></tr><tr><td>Administer</td><td>{admins}</td></tr><tr><td>Run as</td><td>{run_as}</td></tr><tr><td>Sessions</td><td>{per} per person, {total} in all</td></tr><tr><td>Idle sessions end after</td><td>{idle} min</td></tr><tr><td>Session ports</td><td>{lo}-{hi}</td></tr><tr><td>Audit log</td><td><code>{log_path}</code></td></tr></table><p class=mute>Edit the server's multiuser.conf and restart it to change these.</p></div><h2>Audit log</h2><div class=card><pre>{log}</pre></div></main>",
                me = esc(me),
                n = all.len(),
                p = people.len(),
                allow = if c.allow_groups.is_empty() { format!("any account at or above uid {}", c.min_uid) } else { format!("members of {}", esc(&c.allow_groups.join(", "))) },
                admins = esc(&admins),
                run_as = esc(&run_as),
                per = c.max_per_user,
                total = c.max_total,
                idle = c.idle.as_secs() / 60,
                lo = c.ports.0,
                hi = c.ports.1,
                log_path = esc(&c.audit_log.display().to_string()),
            ),
        ),
    )
}

/// `/s/<id>/rest` → (id, rest).
fn session_route(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/s/")?;
    let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
    (id.len() == 10 && id.chars().all(|c| c.is_ascii_hexdigit())).then_some((id, tail))
}

/// The request, passed to the session with its token; the answer passed back.
/// Connection: close, so the answer ends with the stream.
fn forward(req: &mut Request, port: u16, token: &str, url: &str) -> Resp {
    let mut body = Vec::new();
    // An upload (Files › Upload, a file attached for the agent) carries a
    // whole file - a PDF of supporting docs is past the 8 MB of an /api/q.
    let cap = if url.starts_with("/upload") { UPLOAD_MAX } else { BODY_MAX };
    let _ = req.as_reader().take(cap).read_to_end(&mut body);
    forward_body(req, port, token, url, body, None)
}

/// `forward`, with the body already read.
/// `banner`, when given, goes into an HTML answer just before `</body>`.
fn forward_body(req: &mut Request, port: u16, token: &str, url: &str, body: Vec<u8>, banner: Option<&str>) -> Resp {
    let method = req.method().as_str().to_owned();
    let ctype = get_header(req, "Content-Type").map(str::to_owned);
    // X-Cove is how a session knows an upload came from Cove's own page
    // (server.rs refuses one without it). It was not passed on, so every
    // upload through cove.ndexr.io got "uploads come from Cove" (2026-10-15).
    // Passed only when the browser's Origin is ours.
    let cove = get_header(req, "X-Cove").filter(|v| *v == "1" && same_origin(req)).map(str::to_owned);
    // The Viewer's /port/: a person's own server, which wants the browser's
    // request as it was (Accept, its own cookies …) and answers with
    // redirects and cookies of its own.
    let viewer = url.starts_with("/port/");
    // A session started a moment ago may not listen yet.
    let until = Instant::now() + Duration::from_secs(15);
    let mut s = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < until => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return html(502, page("session", format!("<main><p>This session is not answering: {}</p><p><a href=/>Back</a></p></main>", esc(&e.to_string())))),
        }
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(660)));
    let mut head = if viewer {
        let mine = get_header(req, "Cookie").unwrap_or("").split(';').map(str::trim).filter(|c| !c.is_empty() && !c.starts_with(&format!("{COOKIE}="))).collect::<Vec<_>>().join("; ");
        let cookie = if mine.is_empty() { format!("cove={token}") } else { format!("cove={token}; {mine}") };
        crate::server::request_head(req, port, url, Some(cookie), Some(body.len()), false).trim_end_matches("\r\n").to_owned() + "\r\n"
    } else {
        format!("{method} {url} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nCookie: cove={token}\r\nContent-Length: {}\r\n", body.len())
    };
    if let Some(c) = ctype.filter(|_| !viewer) {
        head.push_str(&format!("Content-Type: {c}\r\n"));
    }
    if let Some(v) = cove {
        head.push_str(&format!("X-Cove: {v}\r\n"));
    }
    head.push_str("\r\n");
    if s.write_all(head.as_bytes()).and_then(|_| s.write_all(&body)).is_err() {
        return html(502, "<p>the session did not take the request</p>".into());
    }
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else { return html(502, "<p>the session gave no answer</p>".into()) };
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status: u16 = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).unwrap_or(502);
    let lines: Vec<&str> = lines.collect();
    // The session answers an HTTP/1.1 request chunked, and Transfer-Encoding
    // is not among the headers passed on — so the body has to be un-chunked
    // here. Passed through raw (until 2026-10-13), every asset carried its
    // chunk-size lines: cove.js began "2000\r\n" and broke mid-identifier
    // every 8 KiB, the browser threw a SyntaxError before fetching the wasm,
    // and every session sat on "loading Cove…" forever.
    let chunked = lines.iter().any(|l| {
        l.split_once(':').is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked"))
    });
    let payload = &raw[split + 4..];
    let mut body = if chunked { dechunk(payload) } else { payload.to_vec() };
    let html_answer = lines.iter().any(|l| l.split_once(':').is_some_and(|(k, v)| k.trim().eq_ignore_ascii_case("content-type") && v.contains("text/html")));
    if let Some(b) = banner.filter(|_| html_answer) {
        let at = body.windows(7).rposition(|w| w.eq_ignore_ascii_case(b"</body>")).unwrap_or(body.len());
        body.splice(at..at, b.bytes());
    }
    let mut r = Response::from_data(body).with_status_code(status);
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim();
            // Only what the page needs; the session's own cookie stays inside.
            let lk = k.to_ascii_lowercase();
            let passed = ["content-type", "cache-control", "content-security-policy", "location"].contains(&lk.as_str())
                || (viewer && ["set-cookie", "content-encoding", "etag", "last-modified", "vary", "content-disposition"].contains(&lk.as_str()));
            if passed {
                r = r.with_header(header(k, v.trim()));
            }
        }
    }
    r
}

/// A chunked HTTP/1.1 body, decoded: `<hex size>[;ext]\r\n<data>\r\n` … `0\r\n\r\n`.
/// Stops at the last-chunk or at anything malformed, keeping what decoded.
fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    while let Some(eol) = b.windows(2).position(|w| w == b"\r\n") {
        let size_field = String::from_utf8_lossy(&b[..eol]);
        let Ok(n) = usize::from_str_radix(size_field.split(';').next().unwrap_or("").trim(), 16) else { break };
        let start = eol + 2;
        if n == 0 || start + n > b.len() {
            break;
        }
        out.extend_from_slice(&b[start..start + n]);
        b = b.get(start + n + 2..).unwrap_or(&[]);
    }
    out
}

/// A lens or a bookmark: open the person's most recent session on a folder,
/// starting one when they have none. The folder of ?domain= is the person's
/// own checkout of it when they have one (~/domains/<host>), else the front
/// proxy's answer (X-Ndexr-Domain-Dir), else their home.
fn open_on(ctx: &Ctx, user: &str, req: &Request, q: &HashMap<String, String>) -> Option<Resp> {
    let folder = if let Some(d) = q.get("domain") {
        let ok = !d.is_empty() && d.len() < 254 && d.contains('.') && !d.contains("..") && d.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c));
        if !ok {
            return Some(home_page(ctx, user, &format!("not a domain: {d}")));
        }
        let a = match account_for(ctx, user) {
            Ok(a) => a,
            Err(e) => return Some(home_page(ctx, user, &e)),
        };
        [format!("{}/domains/{d}", a.home), get_header(req, "X-Ndexr-Domain-Dir").unwrap_or("").to_owned()].into_iter().find(|p| p.starts_with('/') && Path::new(p).is_dir()).unwrap_or_else(|| a.home.clone())
    } else {
        let f = q.get("folder")?;
        if !f.starts_with('/') || f.contains("/../") {
            return Some(home_page(ctx, user, "a folder is an absolute path"));
        }
        f.clone()
    };
    let folder = same_folder(&folder);
    // The host decides, never ?editor=: a Cove lens opens Cove (2026-10-18).
    let kind = host_kind(req);
    if kind.direct() {
        return Some(open_direct_on(ctx, user, &folder, kind));
    }
    Some(match latest_or_new(ctx, user) {
        Ok(id) => redirect(&format!("/s/{id}/?folder={}", encode(&folder))),
        Err(e) => home_page(ctx, user, &e),
    })
}

/// The first minute after sign-up (operator, 2026-10-15, after a new user
/// could not get in: "it just needs to be more graceful for users - when they
/// get logged in for the first time"). Claiming a username at console starts
/// two things Cove waits on: ood-sync-users teaching nginx the name (a
/// one-minute timer; until then the header is their email), and the unix
/// account in the ndexr group. Either missing used to be an error page.
fn signing_up(ctx: &Ctx, req: &Request) -> bool {
    let Auth::Proxy { header, key } = &ctx.conf.auth else { return false };
    get_header(req, "X-Cove-Key").is_some_and(|k| same(k.trim(), key)) && get_header(req, header).is_some_and(|u| u.contains('@'))
}

/// The account is not there yet, or not yet in an allowed group. Anything
/// else account_for refuses (a system account, no shell) is not waited out.
fn provisioning(ctx: &Ctx, user: &str) -> bool {
    let as_ = ctx.conf.run_as.get(user).map(String::as_str).unwrap_or(user);
    match account(as_) {
        None => true,
        Some(a) => !ctx.conf.allow_groups.is_empty() && !in_any(&a, &ctx.conf.allow_groups),
    }
}

/// The person's own site (<user>.ndexr.io), else their first domain.
fn own_domain(ctx: &Ctx, user: &str) -> Option<String> {
    let doms = person_domains(user, &account_for(ctx, user).ok()?);
    let own = format!("{user}.ndexr.io");
    doms.iter().find(|(h, _)| *h == own).or(doms.first()).map(|(_, p)| p.clone())
}

/// Cove's ⌂ ndexr project is ~/ndexr, and no provisioning step made it: a person
/// who opened it got "500 /home/claude/ndexr: No such file or directory"
/// (2026-10-30). Built at login by its one writer, in the background: it is
/// idempotent, and they are on their own site meanwhile.
fn ensure_own_ndexr(ctx: &Ctx, user: &str) {
    let Ok(a) = account_for(ctx, user) else { return };
    if Path::new(&a.home).join("ndexr").exists() {
        return;
    }
    let script = crate::host::platform_root().join("scripts/cells/tenant-ndexr.sh");
    let user = user.to_owned();
    std::thread::spawn(move || {
        let _ = std::process::Command::new("runuser").args(["-u", "ubuntu", "--", "bash"]).arg(script).arg(user).current_dir("/").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    });
}

/// This person's Cove session `id` runs an older build than the current one.
fn session_is_stale(ctx: &Ctx, user: &str, id: &str) -> bool {
    let (now, ..) = current_release(ctx);
    let running = ctx.sessions.lock().unwrap().get(id).filter(|s| s.owner == user && s.kind == Kind::Ide).map(|s| s.release.clone());
    !now.is_empty() && running.is_some_and(|r| r != now)
}

/// Their own site still says "Your site" and its thread is empty: nobody has
/// told it what it is (ide.rs `own_site_first` asks the same of the client).
fn own_site_waiting(ctx: &Ctx, user: &str) -> bool {
    let Some(dir) = own_domain(ctx, user) else { return false };
    let host = Path::new(&dir).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let named = std::fs::read_to_string(Path::new(&dir).join("CONTEXT.md")).is_ok_and(|t| t.lines().next() == Some(format!("# {} · {host}", crate::ui::agent::SITE_TITLE).as_str()));
    named && crate::host::agent_history_as(user, &host).is_ok_and(|t| t.is_empty())
}

/// Holds the person on the URL they came in on and reloads it until the
/// account is ready, then `/` takes them in as usual. After a couple of
/// minutes it says where to go if nothing is coming (no username picked).
fn setting_up_page(user: Option<&str>) -> Resp {
    let what = user.map(|u| format!("Your account and <b>{}.ndexr.io</b> are", esc(u))).unwrap_or_else(|| "Your account is".into());
    html(
        200,
        page(
            "setting up",
            format!(
                "<main style='text-align:center;padding-top:18vh'><h2 style='font-weight:500'>Setting up your workspace</h2>\
<p>{what} being created. This takes about a minute; Cove opens on its own when it is ready.</p>\
<p class=mute id=t>&nbsp;</p>\
<p class=mute id=late hidden>Still waiting? If you have not picked a username yet, finish signing up at <a href='https://console.ndexr.io/'>console.ndexr.io</a>.</p>\
<script>(function(){{var k='cove-setup-since',n=Date.now(),t=n;try{{t=+sessionStorage.getItem(k)||n;if(n-t>9e5)t=n;sessionStorage.setItem(k,t)}}catch(e){{}}\
var s=Math.round((n-t)/1000);document.getElementById('t').textContent=s?'Waiting '+s+'s':'';if(s>120)document.getElementById('late').hidden=false;\
setTimeout(function(){{location.reload()}},4000)}})()</script></main>"
            ),
        ),
    )
}

/// The person's most recent Cove session, or a new one when they have none:
/// the stack's newest environment, so R and Python are both there from the
/// first click.
fn latest_or_new(ctx: &Ctx, user: &str) -> Result<String, String> {
    match rows(ctx, Some(user)).into_iter().find(|r| r.9 == Kind::Ide).map(|r| r.1) {
        Some(id) => Ok(id),
        None => new_session(ctx, user, "", &default_module(ctx), "", Kind::Ide),
    }
}

/// One spelling per folder, so a bookmark with a trailing slash or through a
/// symlink finds the session already on it instead of starting another
/// (open_direct_on matches folders exactly). Resolved as root, which only
/// reads the path; a folder that does not resolve keeps its tidied spelling.
fn same_folder(folder: &str) -> String {
    let tidy = {
        let mut t = folder.to_owned();
        while t.contains("//") {
            t = t.replace("//", "/");
        }
        let t = t.trim_end_matches('/');
        if t.is_empty() { "/".to_owned() } else { t.to_owned() }
    };
    std::fs::canonicalize(&tidy).ok().filter(|p| p.is_dir()).map(|p| p.display().to_string()).unwrap_or(tidy)
}

/// RStudio cannot change folder under a running session the way the ide
/// does, so a folder IS a session: the person's RStudio (or Positron) already
/// on it, else a new one started there. The URL is the workspace either way.
fn open_direct_on(ctx: &Ctx, user: &str, folder: &str, kind: Kind) -> Resp {
    let existing = {
        let mut all = ctx.sessions.lock().unwrap();
        all.values_mut().find_map(|s| (s.owner == user && s.kind == kind && s.project == folder && alive(s)).then(|| s.id.clone()))
    };
    if let Some(r) = existing.and_then(|id| enter_session(ctx, user, &id, "")) {
        return r;
    }
    let name: String = Path::new(folder).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // The folder's own environment first (.ndexr-env), as Cove's domains do.
    let module = crate::host::envs::pinned(Path::new(folder)).unwrap_or_else(|| default_module(ctx));
    new_and_enter(ctx, user, &name, &module, folder, kind)
}

/// A new RStudio or Positron session, entered.
fn new_and_enter(ctx: &Ctx, user: &str, name: &str, module: &str, folder: &str, kind: Kind) -> Resp {
    match new_session(ctx, user, name, module, folder, kind) {
        Ok(id) => enter_session(ctx, user, &id, "").unwrap_or_else(|| home_page_for(ctx, user, "the session ended as it started", kind)),
        Err(e) => home_page_for(ctx, user, &e, kind),
    }
}

/// The person's most recent Positron session, else a new one in their home
/// (2026-10-14: positron.ndexr.io takes the signed-in person straight in, as
/// cove.ndexr.io does).
fn latest_or_new_positron(ctx: &Ctx, user: &str) -> Resp {
    let latest = rows(ctx, Some(user)).into_iter().find(|r| r.9 == Kind::Positron).map(|r| r.1);
    latest.and_then(|id| enter_session(ctx, user, &id, "")).unwrap_or_else(|| new_and_enter(ctx, user, "", &default_module(ctx), "", Kind::Positron))
}

/// `/r/<id>/rest` or `/p/<id>/rest` → (kind, id, rest).
fn direct_route(path: &str) -> Option<(Kind, &str, &str)> {
    let (kind, rest) = if let Some(r) = path.strip_prefix("/r/") {
        (Kind::RStudio, r)
    } else {
        (Kind::Positron, path.strip_prefix("/p/")?)
    };
    let (id, tail) = rest.split_once('/').unwrap_or((rest, ""));
    (id.len() == 10 && id.chars().all(|c| c.is_ascii_hexdigit())).then_some((kind, id, tail))
}

/// nginx's auth_request for every /r/<id>/ and /p/<id>/ request
/// (conf.d/rstudio_sessions.conf, conf.d/positron_sessions.conf):
/// 200 + X-Rs-Port when the request carries that session's cookie and
/// the session is alive, else 401 - and nginx then sends the browser through
/// the signed-in front door, which hands the owner a fresh cookie.
fn rstudio_gate(ctx: &Ctx, req: &Request) -> Resp {
    let deny = || Response::from_data(Vec::new()).with_status_code(401);
    // Only nginx asks: the same key as the person header.
    if let Auth::Proxy { key, .. } = &ctx.conf.auth {
        if !get_header(req, "X-Cove-Key").is_some_and(|k| same(k.trim(), key)) {
            return deny();
        }
    }
    let uri = get_header(req, "X-Original-URI").unwrap_or("");
    let Some((kind, id, _)) = direct_route(uri.split('?').next().unwrap_or("")) else { return deny() };
    let Some(cap) = cookie_named(req, &format!("ndexr_rs_{id}")) else { return deny() };
    let mut all = ctx.sessions.lock().unwrap();
    let Some(s) = all.get_mut(id) else { return deny() };
    if s.kind != kind || !same(&cap, &s.token) || !alive(s) {
        return deny();
    }
    s.last = Instant::now();
    Response::from_data(Vec::new()).with_status_code(200).with_header(header("X-Rs-Port", &s.port.to_string()))
}

fn body_form(req: &mut Request) -> HashMap<String, String> {
    let mut body = Vec::new();
    let _ = req.as_reader().take(64 << 10).read_to_end(&mut body);
    form(&body)
}

/// The people who have opened Cove - a Cove `session-start` in the audit log -
/// most recent first, with when. Only they can be acted as: someone who never
/// opened it has nothing here to see.
fn cove_people(ctx: &Ctx) -> Vec<(String, String)> {
    let log = std::fs::read_to_string(&ctx.conf.audit_log).unwrap_or_default();
    let mut out: Vec<(String, String)> = Vec::new();
    for l in log.lines().rev() {
        let mut f = l.split_whitespace();
        let (Some(at), Some(who), Some("session-start")) = (f.next(), f.next(), f.next()) else { continue };
        // "<id> Cove \"name\" as …": the kind follows the session id.
        if f.nth(1) == Some(Kind::Ide.label()) && valid_user(who) && !out.iter().any(|(u, _)| u == who) {
            out.push((who.to_owned(), at.to_owned()));
        }
    }
    out
}

/// Start acting as `f["user"]`: a Cove session of theirs, as their account,
/// opened on their own domain - a fresh one, named for the admin, so the
/// person's own session is left as it is (their latest when they are at
/// their limit).
fn act_as(ctx: &Ctx, me: &str, f: &HashMap<String, String>) -> Resp {
    let who = f.get("user").map(|u| u.trim().to_owned()).unwrap_or_default();
    if who == me || !valid_user(&who) || !cove_people(ctx).iter().any(|(u, _)| *u == who) {
        return admin_page(ctx, me, "Pick someone who has opened Cove.");
    }
    let id = match new_session(ctx, &who, "admin view", &default_module(ctx), "", Kind::Ide).or_else(|_| latest_or_new(ctx, &who)) {
        Ok(id) => id,
        Err(e) => return admin_page(ctx, me, &format!("Could not open Cove as {who}: {e}")),
    };
    audit(ctx, me, "act-as", &format!("{who} session {id}"));
    let to = match own_domain(ctx, &who) {
        Some(dir) => format!("/s/{id}/?folder={}", encode(&dir)),
        None => format!("/s/{id}/"),
    };
    redirect(&to).with_header(header("Set-Cookie", &format!("{ACT_AS}={who}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=28800")))
}

/// What sits on every Cove page while an admin acts as someone.
fn acting_banner(who: &str, me: &str) -> String {
    format!(
        "<div style='position:fixed;left:8px;bottom:8px;z-index:2147483647;background:#e8a75d;color:#15171c;font:600 12px system-ui,sans-serif;padding:6px 10px;border-radius:4px;box-shadow:0 2px 8px #0008'>\
         Acting as {who} <span style='font-weight:400'>(you are {me})</span> · <a href='/admin/as/stop' style='color:#15171c'>stop</a></div>",
        who = esc(who),
        me = esc(me)
    )
}

fn handle(ctx: &Ctx, mut req: Request) {
    let url = req.url().to_owned();
    let path = url.split('?').next().unwrap_or("/").to_owned();
    let method = req.method().clone();
    let pam = matches!(ctx.conf.auth, Auth::Pam);
    // A certificate for a name, asked by the server's front (Caddy's on-demand
    // TLS, `cove setup-front`) before it issues one: yes only for this
    // server's own name, COVE_NAME in /etc/cove/multiuser.env - an
    // organisation's Cove at its own address (2026-11-24).
    if path == "/__tls/ask" {
        let asked = url
            .split_once('?')
            .map(|(_, q)| q)
            .unwrap_or("")
            .split('&')
            .find_map(|kv| kv.strip_prefix("domain="))
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let ours = std::env::var("COVE_NAME").is_ok_and(|n| !asked.is_empty() && n.trim().eq_ignore_ascii_case(&asked));
        let _ = req.respond(Response::from_string(if ours { "yes" } else { "not a name this Cove serves" }).with_status_code(if ours { 200 } else { 404 }));
        return;
    }
    if path == "/__rstudio_gate" {
        let resp = rstudio_gate(ctx, &req);
        let _ = req.respond(resp);
        return;
    }
    // A request from someone not signed in: the one write the public view takes.
    if path == "/view/request" {
        let resp = crate::server::anon_request_response(&mut req);
        let _ = req.respond(resp);
        return;
    }
    // Cove as an MCP server for Claude (mcp.rs): its discovery documents,
    // registration, tokens and the endpoint itself are called by Claude's
    // servers, with no Cove sign-in - the endpoint takes a bearer token only.
    if crate::server::mcp::is_open_path(&path) {
        let base = crate::server::mcp::base(get_header(&req, "X-Forwarded-Host").or_else(|| get_header(&req, "Host")));
        let mut body = Vec::new();
        let _ = req.as_reader().take(BODY_MAX).read_to_end(&mut body);
        let resp = match path.as_str() {
            p if p.starts_with("/.well-known/oauth-protected-resource") => crate::server::mcp::protected_resource(&base),
            p if p.starts_with("/.well-known/oauth-authorization-server") => crate::server::mcp::authorization_server(&base),
            "/mcp/register" if method == Method::Post => crate::server::mcp::register(&body),
            "/mcp/token" if method == Method::Post => crate::server::mcp::token(&form(&body)),
            "/mcp" | "/mcp/" => crate::server::mcp::endpoint(&base, method.as_str(), get_header(&req, "Authorization"), &body, &McpHost(ctx)),
            _ => Response::from_data(b"not found".to_vec()).with_status_code(404),
        };
        let _ = req.respond(resp);
        return;
    }
    // The public view needs no sign-in and gets no session (server.rs).
    if let Some(resp) = crate::server::public_response(&req, &ctx.web, &url) {
        let _ = req.respond(resp);
        return;
    }
    let real = caller(ctx, &req);
    // An admin acting as someone is that person everywhere but /admin.
    let acting = real.as_deref().filter(|r| is_admin(ctx, r)).and_then(|r| cookie_named(&req, ACT_AS).filter(|u| valid_user(u) && u != r));
    let banner = acting.as_deref().zip(real.as_deref()).map(|(a, r)| acting_banner(a, r));
    let who = if path == "/admin" || path.starts_with("/admin/") { real.clone() } else { acting.clone().or_else(|| real.clone()) };
    let resp = match (method, path.as_str(), who) {
        (Method::Get, "/login", _) if pam => login_page(""),
        (Method::Post, "/login", _) if pam && same_origin(&req) => sign_in(ctx, &mut req),
        (Method::Get, "/logout", _) if pam => {
            if let Some(sid) = cookie(&req) {
                ctx.cookies.lock().unwrap().remove(&sid);
            }
            redirect("/login").with_header(header("Set-Cookie", &format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0")))
        }
        (_, _, None) => {
            if !pam && *req.method() == Method::Get && !path.contains("/api/") && signing_up(ctx, &req) {
                setting_up_page(None)
            } else if !pam && *req.method() == Method::Get && !path.contains("/api/") {
                // Not signed in: to console.ndexr.io's sign-in, and back to
                // this same page (operator, 2026-10-25: "if a session hasnt
                // signed into cove it should get sent to console.ndexr.io to
                // login and then back"). It said "not signed in" in plain text.
                redirect(&sign_in_url(&req, &url))
            } else if !pam {
                Response::from_data(b"not signed in: this server sits behind a sign-in proxy".to_vec()).with_status_code(401)
            } else if path.contains("/api/") {
                Response::from_data(b"sign in at /login".to_vec()).with_status_code(401)
            } else {
                redirect("/login")
            }
        }
        // Straight in (operator, 2026-10-14: "the site never opens with the
        // ide selector, just takes the logged in user to the site"): a bare
        // cove.ndexr.io is the person's latest session, a new one if none.
        // A Cove host has no list (2026-10-18): the person's other sessions
        // are its Recent menu. An rstudio.* host keeps its list as its
        // front, where the RStudio sessions are picked; a positron.* host
        // goes straight in, like Cove.
        // A first sign-in can beat the account (see setting_up_page): wait
        // here, on whatever URL they came in on, rather than fail.
        (Method::Get, "/", Some(user)) if provisioning(ctx, &user) => setting_up_page(Some(&user)),
        (Method::Get, "/", Some(user)) => {
            let q = query(&url);
            let fresh = !rows(ctx, Some(&user)).iter().any(|r| r.9 == Kind::Ide);
            open_on(ctx, &user, &req, &q).unwrap_or_else(|| match host_kind(&req) {
                // Someone's first Cove opens on their own site, the domain
                // console made for them at sign-up, not on a bare home folder.
                Kind::Ide => match latest_or_new(ctx, &user) {
                    Ok(id) => {
                        // A site waiting for its first answer needs the build
                        // that asks: this login moves their session onto it
                        // (operator, 2026-10-30: "each person on next log in").
                        ensure_own_ndexr(ctx, &user);
                        let waiting = own_site_waiting(ctx, &user);
                        if waiting && session_is_stale(ctx, &user, &id) {
                            let _ = update_session(ctx, &user, &id);
                        }
                        match own_domain(ctx, &user).filter(|_| fresh || waiting) {
                            Some(dir) => redirect(&format!("/s/{id}/?folder={}", encode(&dir))),
                            None => redirect(&format!("/s/{id}/")),
                        }
                    }
                    Err(e) => home_page(ctx, &user, &e),
                },
                Kind::RStudio => home_page_for(ctx, &user, "", Kind::RStudio),
                Kind::Positron => latest_or_new_positron(ctx, &user),
            })
        }
        // Old bookmarks of the list land in Cove.
        // Connecting Claude (mcp.rs): the person, signed in, says yes.
        (Method::Get, "/mcp/authorize", Some(user)) => crate::server::mcp::authorize_page(&user, &query(&url)),
        (Method::Post, "/mcp/authorize", Some(user)) if same_origin(&req) => {
            let f = body_form(&mut req);
            crate::server::mcp::authorize_decide(&user, &f, &McpHost(ctx))
        }
        (Method::Get, "/sessions", Some(_)) if !host_kind(&req).direct() => redirect("/"),
        (Method::Get, "/sessions", Some(user)) => home_page_for(ctx, &user, "", host_kind(&req)),
        // New is the rstudio.* front's form; Cove starts its own (latest_or_new).
        (Method::Post, "/new", Some(_)) if !host_kind(&req).direct() => redirect("/"),
        (Method::Post, "/new", Some(user)) if same_origin(&req) => {
            let f = body_form(&mut req);
            let g = |k: &str| f.get(k).map(|v| v.trim().to_owned()).unwrap_or_default();
            let kind = host_kind(&req);
            // Only one of the person's domains (home only for someone with none):
            // the form offers nothing else, and a hand-made post gets no further.
            let doms = account_for(ctx, &user).map(|a| workable_domains(&user, &a)).unwrap_or_default();
            let place_ok = if doms.is_empty() { g("project").is_empty() } else { doms.iter().any(|(_, p)| *p == g("project")) };
            if !place_ok {
                home_page_for(ctx, &user, "start in one of your domains", kind)
            } else {
                new_and_enter(ctx, &user, &g("name"), &g("module"), &g("project"), kind)
            }
        }
        (Method::Post, "/quit", Some(user)) if same_origin(&req) => {
            let id = body_form(&mut req).get("id").cloned().unwrap_or_default();
            let owned = ctx.sessions.lock().unwrap().get(&id).is_some_and(|s| s.owner == user);
            if owned {
                stop(ctx, &id, &user, "quit");
            }
            redirect("/sessions")
        }
        (Method::Get, "/admin", Some(user)) if is_admin(ctx, &user) => admin_page(ctx, &user, ""),
        (Method::Post, "/admin/as", Some(user)) if is_admin(ctx, &user) && same_origin(&req) => {
            let f = body_form(&mut req);
            act_as(ctx, &user, &f)
        }
        (_, "/admin/as/stop", Some(user)) => {
            if let Some(a) = &acting {
                audit(ctx, &user, "act-as-stop", a);
            }
            redirect("/admin").with_header(header("Set-Cookie", &format!("{ACT_AS}=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0")))
        }
        (Method::Post, "/admin", Some(user)) if is_admin(ctx, &user) && same_origin(&req) => {
            let f = body_form(&mut req);
            let note = if let Some(id) = f.get("stop") {
                if stop(ctx, id, &user, "stopped by an admin") {
                    format!("Stopped session {id}.")
                } else {
                    format!("No session {id}.")
                }
            } else if let Some(p) = f.get("stop_user").filter(|p| valid_user(p)) {
                let ids: Vec<String> = ctx.sessions.lock().unwrap().values().filter(|s| &s.owner == p).map(|s| s.id.clone()).collect();
                for id in &ids {
                    stop(ctx, id, &user, "stopped by an admin");
                }
                format!("Stopped {} session(s) of {p}.", ids.len())
            } else {
                String::new()
            };
            admin_page(ctx, &user, &note)
        }
        (_, "/admin", Some(_)) => html(403, page("admin", "<main><p>Only administrators see this page. <a href=/>Back</a></p></main>".into())),
        // An RStudio or Positron URL whose gate failed (no cookie yet, or it
        // expired): nginx brings it here signed in. The owner gets the cookie
        // and goes back to exactly where they were; a session that has ended
        // says so.
        (_, p, Some(user)) if direct_route(p).is_some() => {
            let (kind, id, tail) = direct_route(p).unwrap();
            let q = url.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();
            let mine = ctx.sessions.lock().unwrap().get(id).is_some_and(|s| s.kind == kind);
            match enter_session(ctx, &user, id, &format!("{tail}{q}")).filter(|_| mine) {
                Some(r) => r,
                None => html(404, page("not found", format!("<main><p>That {} session has ended. <a href=/>Back</a> - or open the project again from its lens.</p></main>", kind.label()))),
            }
        }
        // A refresh counts as coming back: a person whose site is waiting for
        // its first answer gets the current build here as they do at `/`.
        (Method::Get, p, Some(user)) if session_route(p).is_some_and(|(id, tail)| tail.is_empty() && session_is_stale(ctx, &user, id) && own_site_waiting(ctx, &user)) => {
            if let Some((id, _)) = session_route(p) {
                let _ = update_session(ctx, &user, id);
            }
            redirect(&url)
        }
        (_, p, Some(user)) => match session_route(p) {
            Some((id, tail)) => {
                let target = {
                    let mut all = ctx.sessions.lock().unwrap();
                    all.get_mut(id).filter(|s| s.owner == user).map(|s| {
                        s.last = Instant::now();
                        (s.port, s.token.clone())
                    })
                };
                let q = url.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();
                match target {
                    // the page needs its folder's trailing slash, so its
                    // relative assets and api/q resolve inside /s/<id>/
                    Some(_) if tail.is_empty() && !p.ends_with('/') => redirect(&format!("/s/{id}/{q}")),
                    // One catalog for everyone, with its ETag and gzip, which
                    // `forward` would drop: answered here, not by the session.
                    Some(_) if tail == "software.json" => crate::server::software_response(&req),
                    // Which Cove this session runs and which is published:
                    // the page offers the newer one (app.rs `release_bar`).
                    Some(_) if tail == "release.json" => {
                        let running = ctx.sessions.lock().unwrap().get(id).map(|s| s.release.clone()).unwrap_or_default();
                        let (current, _, _) = current_release(ctx);
                        // What changed (app/WHATSNEW.md): from this session's build to
                        // the published one, and - asked with ?since= - from the
                        // build the page last saw to this one.
                        let news = std::fs::read_to_string(std::path::Path::new(crate::host::PLATFORM_DOMAINS).join("drennanfreddy/cove.ndexr.io/app/WHATSNEW.md")).unwrap_or_default();
                        let since = q.trim_start_matches('?').split('&').find_map(|kv| kv.strip_prefix("since=")).unwrap_or("").to_owned();
                        let notes = crate::ui::release::notes(&news, &running, &current);
                        let since_notes = if since.is_empty() { Vec::new() } else { crate::ui::release::notes(&news, &since, &running) };
                        let body = serde_json::json!({ "running": running, "current": current, "newer": !current.is_empty() && running != current, "notes": notes, "since_notes": since_notes });
                        Response::from_data(body.to_string().into_bytes()).with_header(Header::from_bytes("Content-Type", "application/json").unwrap()).with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
                    }
                    // Cove's Recent menu (recents.rs): the person's Cove
                    // sessions, most recently used first.
                    Some(_) if tail == "sessions.json" => {
                        let list: Vec<serde_json::Value> = rows(ctx, Some(&user))
                            .into_iter()
                            .filter(|r| r.9 == Kind::Ide)
                            .map(|r| serde_json::json!({ "id": r.1, "name": r.2, "module": r.4, "project": r.5, "active": r.8, "current": r.1 == id }))
                            .collect();
                        Response::from_data(serde_json::Value::from(list).to_string().into_bytes()).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store"))
                    }
                    // End one of the others from Recent; not the one asking.
                    Some(_) if tail == "end" && *req.method() == Method::Post => {
                        let other = body_form(&mut req).get("id").cloned().unwrap_or_default();
                        let owned = other != id && ctx.sessions.lock().unwrap().get(&other).is_some_and(|s| s.owner == user && s.kind == Kind::Ide);
                        if get_header(&req, "X-Cove") != Some("1") || !same_origin(&req) {
                            html(403, page("end", "<main><p>Sessions are ended from Cove.</p></main>".into()))
                        } else if owned && stop(ctx, &other, &user, "quit") {
                            Response::from_data(b"{\"ok\":true}".to_vec()).with_header(header("Content-Type", "application/json"))
                        } else {
                            Response::from_data(b"no such session of yours".to_vec()).with_status_code(404)
                        }
                    }
                    Some(_) if tail == "update" && *req.method() == Method::Post => {
                        if get_header(&req, "X-Cove") != Some("1") || !same_origin(&req) {
                            html(403, page("update", "<main><p>Updates are asked for from Cove.</p></main>".into()))
                        } else {
                            match update_session(ctx, &user, id) {
                                Ok(_) => Response::from_data(b"{\"ok\":true}".to_vec()).with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
                                Err(e) => Response::from_data(e.into_bytes()).with_status_code(500),
                            }
                        }
                    }
                    // Read with the platform's own database login, which only
                    // this (root) server can see - the session, as the person, cannot.
                    Some(_) if tail == "builds.json" => crate::server::builds_response(),
                    // The canopy as THIS person may see it: their login,
                    // admin and owner-only sites as well as the public ones
                    // (host.rs visible_domains). The public page's
                    // /view/domains.json is the not-signed-in answer.
                    Some(_) if tail == "domains.json" => match crate::host::visible_domains(Some(&user)) {
                        Ok(b) => Response::from_data(b).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-cache")),
                        Err(e) => Response::from_data(e.into_bytes()).with_status_code(503),
                    },
                    // The build machines' logs live under /srv/ndexr/host_runs,
                    // which the session, as the person, may not read.
                    Some(_) if tail == "buildlog.json" => crate::server::build_log_response(&url),
                    // Speech for an agent box, transcribed here as the person
                    // (host.rs transcribe_as): the session, as them, cannot
                    // read the platform's Whisper key.
                    Some(_) if tail == "transcribe" && *req.method() == Method::Post => {
                        let ours = get_header(&req, "X-Cove") == Some("1") && same_origin(&req);
                        let mut body = String::new();
                        let _ = req.as_reader().take(40 << 20).read_to_string(&mut body);
                        if !ours {
                            html(403, page("transcribe", "<main><p>Recordings come from Cove.</p></main>".into()))
                        } else {
                            match crate::host::transcribe_as(&user, &body) {
                                Ok(t) => Response::from_data(serde_json::json!({ "text": t }).to_string().into_bytes()).with_header(Header::from_bytes("Content-Type", "application/json").unwrap()),
                                Err(e) => Response::from_data(e.into_bytes()).with_status_code(500),
                            }
                        }
                    }
                    // A websocket (the Viewer's /port/ - shiny, quarto
                    // preview's reload): through to the session as it is.
                    Some((port, token)) if req.headers().iter().any(|h| h.field.equiv("Upgrade")) => {
                        let mine = get_header(&req, "Cookie").unwrap_or("").split(';').map(str::trim).filter(|c| !c.is_empty() && !c.starts_with(&format!("{COOKIE}="))).collect::<Vec<_>>().join("; ");
                        let cookie = if mine.is_empty() { format!("cove={token}") } else { format!("cove={token}; {mine}") };
                        crate::server::tunnel(req, port, &format!("/{tail}{q}"), Some(cookie));
                        return;
                    }
                    // Some questions only this server can answer for the person.
                    Some((port, token)) if tail == "api/q" && *req.method() == Method::Post => {
                        let mut body = Vec::new();
                        let _ = req.as_reader().take(BODY_MAX).read_to_end(&mut body);
                        match on_behalf(ctx, &user, id, &body) {
                            Some(r) => r,
                            None => forward_body(&mut req, port, &token, &format!("/{tail}{q}"), body, None),
                        }
                    }
                    Some((port, token)) if banner.is_some() && (tail.is_empty() || tail == "index.html") && *req.method() == Method::Get => {
                        forward_body(&mut req, port, &token, &format!("/{tail}{q}"), Vec::new(), banner.as_deref())
                    }
                    Some((port, token)) => forward(&mut req, port, &token, &format!("/{tail}{q}")),
                    None => html(404, page("not found", "<main><p>No such session of yours - it may have ended. <a href=/>Back</a></p></main>".into())),
                }
            }
            None => html(404, page("not found", "<main><p>Nothing here. <a href=/>Back</a></p></main>".into())),
        },
    };
    let _ = req.respond(resp);
}

pub fn main(args: &[String]) -> i32 {
    let mut listen = "127.0.0.1:8787".to_owned();
    let mut web: Option<PathBuf> = None;
    let mut conf_file = PathBuf::from("/etc/cove/multiuser.conf");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--listen" => listen = it.next().cloned().unwrap_or(listen),
            "--web-dir" => web = it.next().map(PathBuf::from),
            "--config" => conf_file = it.next().map(PathBuf::from).unwrap_or(conf_file),
            "-h" | "--help" => {
                println!("cove multiuser [--listen 127.0.0.1:8787] [--web-dir DIR] [--config /etc/cove/multiuser.conf]");
                return 0;
            }
            other => {
                eprintln!("multiuser: unknown argument {other}");
                return 2;
            }
        }
    }
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("multiuser: runs as root, to sign people in and start each session as them (for one user, use `cove serve`)");
        return 1;
    }
    let conf = match std::fs::read_to_string(&conf_file) {
        Ok(t) => match parse_conf(&t) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("multiuser: {}: {e}", conf_file.display());
                return 1;
            }
        },
        Err(_) => Conf::default(),
    };
    let web = web.unwrap_or_else(crate::server::default_web_dir);
    if !web.join("index.html").is_file() {
        eprintln!("multiuser: no UI at {} (index.html) - pass --web-dir", web.display());
        return 1;
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("multiuser: {e}");
            return 1;
        }
    };
    let server = match Server::http(&listen) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("multiuser: cannot listen on {listen}: {e}");
            return 1;
        }
    };
    println!(
        "cove multiuser on http://{listen}/ · {} · {} sessions per person, {} in all · idle {} min · audit {}",
        match &conf.auth {
            Auth::Pam => format!("PAM {}", pam_service()),
            Auth::Proxy { header, .. } => format!("proxy auth ({header} + key)"),
        },
        conf.max_per_user,
        conf.max_total,
        conf.idle.as_secs() / 60,
        conf.audit_log.display()
    );
    // Before any session is started or adopted: one group per account, under
    // this server's delegated cgroup.
    let people = match crate::host::limits::people_root() {
        Ok(p) => {
            println!("multiuser: {} ({})", conf.person.describe(), p.display());
            Some(p)
        }
        Err(e) => {
            println!("multiuser: nobody is limited - {e}");
            None
        }
    };
    let ctx = Arc::new(Ctx { conf, web, exe, sessions: Default::default(), cookies: Default::default(), failures: Default::default(), modules: Default::default(), audit: Default::default(), people });
    reaper(ctx.clone());
    {
        // Domains shared to work on: the units match the database within 20 s
        // of any change, wherever it was made (a platform session writes
        // shares itself), and again after a reboot.
        let ctx = ctx.clone();
        let (kick, woken) = std::sync::mpsc::channel();
        let _ = SHARES_KICK.set(kick);
        std::thread::spawn(move || loop {
            shares_reconcile(&ctx);
            let _ = woken.recv_timeout(Duration::from_secs(20));
            while woken.try_recv().is_ok() {}
        });
    }
    {
        // Everyone with a Cove session, measured against their plan
        // (storage.rs). RStudio and Positron sessions have no Cove to show it.
        let ctx = ctx.clone();
        crate::host::storage::keep::keeper(move || {
            let mut out: Vec<crate::host::storage::keep::Person> = Vec::new();
            for s in ctx.sessions.lock().unwrap().values().filter(|s| s.kind == Kind::Ide) {
                if !out.iter().any(|p| p.owner == s.owner) {
                    out.push(crate::host::storage::keep::Person { owner: s.owner.clone(), home: PathBuf::from(&s.account.home), gid: s.account.gid });
                }
            }
            out
        });
    }
    {
        // The server going down leaves every session running, written down,
        // for the next one to take back (`adopt`) - a build of the working
        // tree restarts it, and that must not end anyone's work. A person's
        // Quit, admin Stop and the idle reaper still end sessions.
        let ctx = ctx.clone();
        crate::server::on_term(move || save_sessions(&ctx));
    }
    let n = adopt(&ctx);
    if n > 0 {
        println!("multiuser: took back {n} running session(s) from the last server");
    }
    // Sessions taken back keep their group; its limits are this config's.
    if let Some(people) = &ctx.people {
        let uids: std::collections::BTreeSet<u32> = ctx.sessions.lock().unwrap().values().map(|s| s.account.uid).collect();
        for uid in uids {
            if let Err(e) = crate::host::limits::place(people, uid, limits_for(&ctx, uid)) {
                eprintln!("multiuser: limits for uid {uid}: {e}");
            }
        }
    }
    let _ = crate::server::LISTEN_PORT.set(server.server_addr().to_ip().map_or(0, |a| a.port()));
    for req in server.incoming_requests() {
        let ctx = ctx.clone();
        std::thread::spawn(move || handle(&ctx, req));
    }
    0
}

#[cfg(test)]
mod tests {

    #[test]
    fn one_spelling_per_folder() {
        let d = std::env::temp_dir();
        let real = d.canonicalize().unwrap().display().to_string();
        assert_eq!(same_folder(&format!("{}/", d.display())), real);
        assert_eq!(same_folder(&format!("{}//", d.display())), real);
        assert_eq!(same_folder("/no/such/folder//x/"), "/no/such/folder/x");
        assert_eq!(same_folder("/"), "/");
    }

    use super::*;

    #[test]
    fn the_config_is_strict() {
        let c = parse_conf("# who\nallow-groups = ndexr-users, lab\nadmin-groups = sudo\nadmin-users = op\nrun-as = op:ubuntu\nmin-uid = 1001\nidle-minutes = 30\nports = 28000-28100\nmax-sessions-per-user = 3\nmax-sessions = 50\n").unwrap();
        assert_eq!(c.allow_groups, ["ndexr-users", "lab"]);
        assert_eq!((c.admin_groups.clone(), c.admin_users.clone()), (vec!["sudo".to_owned()], vec!["op".to_owned()]));
        assert_eq!(c.run_as.get("op").map(String::as_str), Some("ubuntu"));
        assert_eq!((c.min_uid, c.idle.as_secs(), c.ports, c.max_per_user, c.max_total), (1001, 1800, (28000, 28100), 3, 50));
        assert_eq!(parse_conf("").unwrap(), Conf::default());
        assert!(parse_conf("allow-group = x").is_err(), "a typo must not widen access");
        assert!(parse_conf("ports = 80-90").is_err());
        assert!(parse_conf("run-as = op:root").is_err());
        assert!(parse_conf("auth = proxy").is_err(), "a header without the proxy's key is anyone's");
        assert!(parse_conf("auth = ldap").is_err());
        let c = parse_conf("person-memory = 6G\nperson-cpus = 4\nperson-tasks = 4096\n").unwrap();
        assert_eq!(c.person, crate::host::limits::Limits { memory: 6 << 30, cpus: 4, tasks: 4096 });
        assert!(parse_conf("person-memory = lots").is_err());
    }

    #[test]
    fn proxy_auth_needs_a_real_key() {
        let d = std::env::temp_dir().join(format!("ndexr-mu-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("short"), "abc").unwrap();
        std::fs::write(d.join("key"), "k".repeat(40)).unwrap();
        assert!(parse_conf(&format!("auth = proxy\nproxy-key-file = {}", d.join("short").display())).is_err());
        let c = parse_conf(&format!("auth = proxy\nproxy-key-file = {}", d.join("key").display())).unwrap();
        assert_eq!(c.auth, Auth::Proxy { header: "X-Remote-User".into(), key: "k".repeat(40) });
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn names_are_login_names() {
        assert!(valid_user("alice") && valid_user("a.b-c_1") && valid_user("_svc"));
        for bad in ["", "Alice", "root;id", "../x", "a b", "-x", &"x".repeat(33)] {
            assert!(!valid_user(bad), "{bad}");
        }
    }

    #[test]
    fn who_may_sign_in() {
        let a = Account { name: "a".into(), uid: 1500, gid: 1500, home: "/home/a".into(), shell: "/bin/bash".into(), groups: vec!["a".into(), "lab".into()] };
        let c = Conf::default();
        assert!(may_sign_in(&a, &c).is_ok());
        assert!(may_sign_in(&Account { uid: 999, ..a.clone() }, &c).is_err());
        assert!(may_sign_in(&Account { shell: "/usr/sbin/nologin".into(), ..a.clone() }, &c).is_err());
        assert!(may_sign_in(&a, &Conf { allow_groups: vec!["other".into()], ..c.clone() }).is_err());
        assert!(may_sign_in(&a, &Conf { allow_groups: vec!["lab".into()], ..c }).is_ok());
    }

    #[test]
    fn session_paths() {
        assert_eq!(session_route("/s/0123456789/api/q"), Some(("0123456789", "api/q")));
        assert_eq!(session_route("/s/0123456789/"), Some(("0123456789", "")));
        assert_eq!(session_route("/s/0123456789"), Some(("0123456789", "")));
        assert_eq!(session_route("/s/../etc/x"), None);
        assert_eq!(session_route("/s/zz/x"), None);
        assert_eq!(session_route("/api/q"), None);
    }

    #[test]
    fn forms_and_dates() {
        let f = form(b"user=alice&pass=p%40ss+word%26x");
        assert_eq!(f["user"], "alice");
        assert_eq!(f["pass"], "p@ss word&x");
        assert_eq!(query("/?domain=a.ndexr.io&x=1")["domain"], "a.ndexr.io");
        assert!(now_utc().starts_with("20") && now_utc().ends_with('Z'));
    }

    #[test]
    fn rstudio_sessions_route_and_parse() {
        assert_eq!(direct_route("/r/0123456789/rpc/client_init"), Some((Kind::RStudio, "0123456789", "rpc/client_init")));
        assert_eq!(direct_route("/r/0123456789"), Some((Kind::RStudio, "0123456789", "")));
        assert_eq!(direct_route("/p/0123456789/stable-x/static/out"), Some((Kind::Positron, "0123456789", "stable-x/static/out")));
        assert_eq!(direct_route("/r/../etc"), None);
        assert_eq!(direct_route("/p/zz/"), None);
        assert_eq!(direct_route("/s/0123456789/"), None);
        assert_eq!(session_route("/r/0123456789/"), None);
        assert_eq!(Kind::parse("RStudio"), Kind::RStudio);
        // Inlet's host, and its old one so links people kept still open it.
        assert_eq!(super::kind_of_host("inlet.ndexr.io"), Kind::RStudio);
        assert_eq!(super::kind_of_host("rstudio.ndexr.io"), Kind::Ide, "retired 2026-10-29");
        assert_eq!(super::kind_of_host("positron.ndexr.io"), Kind::Positron);
        assert_eq!(super::kind_of_host("cove.ndexr.io"), Kind::Ide);
        assert_eq!(Kind::RStudio.label(), "Inlet");
        assert_eq!(Kind::parse("positron"), Kind::Positron);
        assert_eq!(Kind::parse("anything"), Kind::Ide);
        assert_eq!(Kind::RStudio.url("0123456789"), "/r/0123456789/");
        assert_eq!(Kind::Positron.url("0123456789"), "/p/0123456789/");
        assert!(parse_conf("positron-launcher = /tmp").unwrap_err().contains("root-owned"));
        // A launcher anyone could rewrite would run as every person.
        let e = parse_conf("rstudio-launcher = /tmp").unwrap_err();
        assert!(e.contains("root-owned"), "{e}");
        assert!(parse_conf("rstudio-launcher = /nonexistent/launcher").is_err());
        assert_eq!(parse_conf("rstudio-launcher = /usr/bin/env").unwrap().rstudio_launcher, Some(PathBuf::from("/usr/bin/env")));
    }

    #[test]
    fn dechunks_session_answers() {
        assert_eq!(dechunk(b"5\r\nhello\r\n7;x=1\r\n, world\r\n0\r\n\r\n"), b"hello, world");
        // 0x2000 bytes split across chunks, as cove.js arrived.
        let big = vec![b'a'; 0x2000 + 3];
        let mut wire = b"2000\r\n".to_vec();
        wire.extend_from_slice(&big[..0x2000]);
        wire.extend_from_slice(b"\r\n3\r\naaa\r\n0\r\n\r\n");
        assert_eq!(dechunk(&wire), big);
        // A truncated stream keeps what fully arrived.
        assert_eq!(dechunk(b"3\r\nabc\r\n9\r\nde"), b"abc");
    }
}
