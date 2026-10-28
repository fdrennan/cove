#!/usr/bin/env bash
# deploy-app.sh <domain> <user> <dir> - a person's Rust app, live (operator,
# 2026-10-19: "those projects need to start live ... a rust web assembly app
# like cove.ndexr.io but minimal, routing and deployment of the app needs to go
# all of the way through for the user").
#
# Run as root by `cove multiuser` (Query::DeployApp), for the signed-in
# person's own domain only. Unlike scripts/new-rust-app.sh (the operator's,
# which runs the binary as the platform account), everything the person wrote
# is built and run AS THE PERSON - the same account their R, terminal and
# agent already run code as:
#
#   1. build: cargo + build-web.sh as <user>, on Cove's published toolchain
#      (/usr/local/lib/cove/rust, read-only) with their own cargo cache;
#   2. serve: /etc/systemd/system/cove-app-<domain>.service, User=<user>, on
#      127.0.0.1:<port> (8820-8899, kept across reruns);
#   3. route: the domain's `location /` to that port behind the owner gate -
#      src/r/nginx/target.r set_domain_target, the one writer that validates,
#      reloads and restores on failure - as the platform account, once the
#      domain's own vhost exists (publish provisions it; this waits for it).
#
# Each step says what it is doing on its own line ("▸ …"); Cove shows them.
set -euo pipefail
domain="${1:?domain}" user="${2:?user}" dir="${3:?dir}"
[[ "$domain" =~ ^[a-z0-9][a-z0-9.-]+\.[a-z]+$ ]] || { echo "not a domain: $domain"; exit 2; }
[[ "$user" =~ ^[a-z_][a-z0-9_-]*$ ]] || { echo "not a user: $user"; exit 2; }
[[ -f "$dir/Cargo.toml" && -x "$dir/build-web.sh" ]] || { echo "$dir is not a Rust app (no Cargo.toml / build-web.sh)"; exit 2; }
ROOT=/home/ubuntu/ndexr
RUST=/usr/local/lib/cove/rust
tc="$(ls -1d "$RUST"/rustup/toolchains/*/ 2>/dev/null | head -1)"
[[ -n "$tc" && -x "$RUST/cargo/bin/wasm-bindgen" ]] || { echo "Cove's Rust toolchain is not published at $RUST yet - the next Cove release puts it there"; exit 1; }
home="$(getent passwd "$user" | cut -d: -f6)"
[[ -d "$home" ]] || { echo "no home for $user"; exit 1; }

as_user() {
  runuser -u "$user" -- env -i HOME="$home" USER="$user" LANG=C.UTF-8 TMPDIR="$home/tmp" \
    CARGO_HOME="$home/.cargo" CARGO_TARGET_DIR="$dir/target" APP_RUST=/nonexistent \
    PATH="${tc}bin:$RUST/cargo/bin:/usr/local/bin:/usr/bin:/bin" bash -c "$1"
}
runuser -u "$user" -- mkdir -p "$home/tmp" "$home/.cargo"

echo "▸ building the server (cargo, as $user)"
as_user "cd '$dir' && cargo build --release 2>&1 | grep -E '^(error|warning: unused)|Compiling app|Finished' || true; test -x target/release/app"
echo "▸ building the browser UI (wasm)"
as_user "cd '$dir' && ./build-web.sh 2>&1 | tail -3"

unit="cove-app-$domain.service"; ufile="/etc/systemd/system/$unit"
port=""
[[ -f "$ufile" ]] && port="$(sed -n 's/.*app serve \([0-9]*\).*/\1/p' "$ufile")"
# The port the project's own app.env names, when nothing else holds it: the
# strip reads APP_PORT, and an app served on another one showed as "down"
# (2026-10-26, ggsdfgg.ndexr.io: app.env 8837, served on 8824).
if [[ -z "$port" && -f "$dir/app.env" ]]; then
  p="$(sed -n 's/^[[:space:]]*APP_PORT[[:space:]]*=[[:space:]]*["'\'']\{0,1\}\([0-9]*\).*/\1/p' "$dir/app.env" | tail -n1)"
  if [[ -n "$p" ]] && ! grep -rqs "127\.0\.0\.1:$p;" "$ROOT/nginx/sites-enabled" && ! ss -ltn "sport = :$p" | grep -q LISTEN; then
    port=$p
  fi
fi
if [[ -z "$port" ]]; then
  for p in $(seq 8820 8899); do
    grep -rqs "127\.0\.0\.1:$p;" "$ROOT/nginx/sites-enabled" && continue
    grep -qs "app serve $p\b" /etc/systemd/system/cove-app-*.service && continue
    ss -ltn "sport = :$p" | grep -q LISTEN && continue
    port=$p; break
  done
fi
[[ -n "$port" ]] || { echo "no free port in 8820-8899"; exit 1; }

echo "▸ starting it on 127.0.0.1:$port"
cat > "$ufile" <<UNIT
[Unit]
Description=$domain - $user's Rust app (Cove, deploy-app.sh)
After=network.target

[Service]
User=$user
WorkingDirectory=$dir
ExecStart=$dir/target/release/app serve $port
Restart=on-failure
RestartSec=3
NoNewPrivileges=yes
PrivateTmp=yes

[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable "$unit" >/dev/null 2>&1
systemctl restart "$unit"
for _ in $(seq 1 40); do curl -fs -o /dev/null "http://127.0.0.1:$port/" && break; sleep 0.5; done
curl -fs -o /dev/null "http://127.0.0.1:$port/" || { journalctl -u "$unit" -n 15 --no-pager; echo "the app did not answer on $port"; exit 1; }
echo "▸ answering on 127.0.0.1:$port"

echo "▸ routing https://$domain/ to it"
for _ in $(seq 1 60); do
  [[ -f "$ROOT/nginx/sites-enabled/$domain" ]] && break
  sleep 5
done
[[ -f "$ROOT/nginx/sites-enabled/$domain" ]] || { echo "the site's vhost is not there yet (publishing still running) - Deploy again in a minute"; exit 1; }
r="$(runuser -u ubuntu -- mktemp -p /home/ubuntu/tmp target.XXXX.R)"
cat > "$r" <<RS
options(box.path = "$ROOT/src")
box::use(t = r/nginx/target)
x <- t\$set_domain_target("$domain", "port:$port", gate = "owner")
cat(x\$message, "\n"); quit(status = if (isTRUE(x\$ok)) 0 else 1)
RS
chown ubuntu "$r"
runuser -u ubuntu -- bash -c "
  cd '$ROOT'; set -a; . ./.env; set +a
  case \"\${POSTGRES_HOST:-}\" in postgres|pgbouncer) export POSTGRES_HOST=127.0.0.1 ;; esac
  [[ \"\${POSTGRES_DIRECT_HOST:-}\" == postgres ]] && export POSTGRES_DIRECT_HOST=127.0.0.1
  export NDEXR_HOST_LOCAL=1 LANG=C.UTF-8 R_LIBS=/opt/ndexr/eblib/x86_64-pc-linux-gnu/R-4.6.1-foss-2024a
  set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; module load R/4.6.1-foss-2024a >/dev/null 2>&1
  Rscript --no-save '$r' 2>&1 | grep -v 'pool>'
" && rc=0 || rc=$?
rm -f "$r"
[[ $rc -eq 0 ]] || { echo "routing failed"; exit $rc; }
echo "✓ live at https://$domain/"
