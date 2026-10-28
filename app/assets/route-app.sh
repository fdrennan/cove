#!/usr/bin/env bash
# route-app.sh <domain> <user> <dir> - point a published domain at its app's
# port, the one step of Up a person's session cannot take (Query::AppRoute).
#
# Run as root by `cove multiuser`, for the signed-in person's own domain only.
# The app itself is built, started and stopped by its own build.sh / up.sh /
# down.sh, as the person (apps.rs; testy.ndexr.io/APP-CONTRACT.md). This reads
# APP_PORT and APP_HEALTH from its app.env, checks the app answers there, and
# routes https://<domain>/ to it behind the gate its access level asks for through
# src/r/nginx/target.r set_domain_target - the one writer that validates,
# reloads and restores on failure - as the platform account.
set -euo pipefail
domain="${1:?domain}" user="${2:?user}" dir="${3:?dir}"
[[ "$domain" =~ ^[a-z0-9][a-z0-9.-]+\.[a-z]+$ ]] || { echo "not a domain: $domain"; exit 2; }
[[ "$user" =~ ^[a-z_][a-z0-9_-]*$ ]] || { echo "not a user: $user"; exit 2; }
ROOT=/home/ubuntu/ndexr
env="$dir/app.env"
[[ -f "$env" ]] || { echo "$domain has no app.env - nothing to route"; exit 2; }
val() { sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*[\"']\{0,1\}\([^\"']*\).*/\1/p" "$env" | tail -n1; }
kind="$(val APP_KIND)" port="$(val APP_PORT)" health="$(val APP_HEALTH)"
[[ "$kind" == port ]] || { echo "$domain is served from module.r - it has no port to route"; exit 0; }
[[ "$port" =~ ^[0-9]{2,5}$ ]] || { echo "app.env has no APP_PORT"; exit 2; }
vhost="$ROOT/nginx/sites-enabled/$domain"
[[ -f "$vhost" ]] || { echo "not published yet - it answers on 127.0.0.1:$port; Publish puts it on the web"; exit 0; }
curl -fs -o /dev/null --max-time 3 "http://127.0.0.1:$port${health:-/}" || { echo "nothing answers on 127.0.0.1:$port${health:-/} - Up first"; exit 1; }
if grep -qE "proxy_pass +http://127\.0\.0\.1:$port[;/]" "$vhost"; then
  echo "https://$domain/ already goes to port $port"
  exit 0
fi
r="$(runuser -u ubuntu -- mktemp -p /home/ubuntu/tmp target.XXXX.R)"
cat > "$r" <<RS
options(box.path = "$ROOT/src")
box::use(t = r/nginx/target, reg = r/domains/registry)
# The gate follows who the owner let in (Project > Access, set-access.sh);
# owner-only when nothing was chosen.
lvl <- tryCatch(reg\$domain_access_level("$domain"), error = function(e) "owner")
gate <- switch(lvl, public = "password", login = "login", admin = "admin", "owner")
x <- t\$set_domain_target("$domain", "port:$port", gate = gate)
cat(x\$message, "\n"); quit(status = if (isTRUE(x\$ok)) 0 else 1)
RS
chown ubuntu "$r"
rc=0
runuser -u ubuntu -- bash -c "
  cd '$ROOT'; set -a; . ./.env; set +a
  case \"\${POSTGRES_HOST:-}\" in postgres|pgbouncer) export POSTGRES_HOST=127.0.0.1 ;; esac
  [[ \"\${POSTGRES_DIRECT_HOST:-}\" == postgres ]] && export POSTGRES_DIRECT_HOST=127.0.0.1
  export NDEXR_HOST_LOCAL=1 LANG=C.UTF-8 R_LIBS=/opt/ndexr/eblib/x86_64-pc-linux-gnu/R-4.6.1-foss-2024a
  set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; module load R/4.6.1-foss-2024a >/dev/null 2>&1
  Rscript --no-save '$r' 2>&1 | grep -v 'pool>'
" || rc=$?
rm -f "$r"
[[ $rc -eq 0 ]] && echo "https://$domain/ now goes to port $port" || echo "routing failed"
exit $rc
