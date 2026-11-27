//! __HOST__: Network Observatory, live network metrics from /proc and /sys.
//! `app serve <port>` serves on 127.0.0.1.

mod log;
mod probe;
mod proc;
mod sampler;
mod server;

/// The name this project was laid out with (Cove fills `__HOST__` in when it
/// makes the project).
pub const LAID: &str = "__HOST__";

/// This app's name: APP_HOST, which .cove/rust/env.sh sets to the project's
/// folder, else the name it was laid out with. Read when it runs, so a binary
/// Cove compiled ahead of time from this starter (a seed) names the project
/// it runs in, not the one it was built for.
pub fn host_name() -> String {
    std::env::var("APP_HOST").ok().filter(|h| !h.trim().is_empty()).unwrap_or_else(|| LAID.to_owned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["serve", port] => match port.parse::<u16>() {
            Ok(p) => server::serve(p),
            Err(_) => die(&format!("not a port: {port}")),
        },
        _ => die("usage: app serve <port>"),
    }
}

fn die(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2)
}
