//! __HOST__: a scientific HTTP API. `app serve <port>` serves on 127.0.0.1.

mod num;
mod server;

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
