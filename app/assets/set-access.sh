#!/usr/bin/env bash
# set-access.sh <domain> <level> - who may open a domain (Query::AppAccess).
#
# Run as root by `cove multiuser`, for a domain the signed-in person owns
# (multiuser.rs checks that first). <level> is the registry's
# domain_registry.access_level: public | login | owner (the owner and the
# people and groups they shared it with, people_sharing.sql) | admin.
#
# Two gates read it, so both move here:
#   - a Shiny module: shell.r reads the registry (60 s cache) on each render;
#   - an app on its own port: the gate is written into its vhost's
#     `location /`, so it is rewritten through src/r/nginx/target.r
#     set_domain_target - the one writer that validates, reloads and restores
#     on failure. public -> "password" (a site password gate with none set
#     admits everyone), login -> "login", owner -> "owner", admin -> "admin".
#     The vhost is changed first: if that fails (the app is down, the vhost is
#     hand-built) the registry is left as it was, so the two never disagree.
set -euo pipefail
domain="${1:?domain}" level="${2:?level}"
[[ "$domain" =~ ^[a-z0-9][a-z0-9.-]+\.[a-z]+$ ]] || { echo "not a domain: $domain"; exit 2; }
case "$level" in public|login|owner|admin) ;; *) echo "not a level: $level"; exit 2 ;; esac
ROOT=/home/ubuntu/ndexr
r="$(runuser -u ubuntu -- mktemp -p /home/ubuntu/tmp access.XXXX.R)"
cat > "$r" <<RS
options(box.path = "$ROOT/src")
box::use(reg = r/domains/registry, t = r/nginx/target)
d <- "$domain"; lvl <- "$level"
cur <- tryCatch(t\$domain_target_of(d), error = function(e) NULL)
if (!is.null(cur) && identical(cur\$kind, "port")) {
  gate <- switch(lvl, public = "password", login = "login", admin = "admin", "owner")
  x <- t\$set_domain_target(d, cur\$label, gate = gate)
  if (!isTRUE(x\$ok)) { cat(x\$message, "\n"); quit(status = 1) }
}
if (!isTRUE(reg\$set_access_level(d, lvl))) { cat(d, ": the registry refused '", lvl, "'\n", sep = ""); quit(status = 1) }
cat(d, " is now ", lvl, "\n", sep = "")
RS
chown ubuntu "$r"
rc=0
runuser -u ubuntu -- bash -c "
  cd '$ROOT'; set -a; . ./.env; set +a
  case \"\${POSTGRES_HOST:-}\" in postgres|pgbouncer) export POSTGRES_HOST=127.0.0.1 ;; esac
  [[ \"\${POSTGRES_DIRECT_HOST:-}\" == postgres ]] && export POSTGRES_DIRECT_HOST=127.0.0.1
  export NDEXR_HOST_LOCAL=1 LANG=C.UTF-8 R_LIBS=/opt/ndexr/eblib/x86_64-pc-linux-gnu/R-4.6.1-foss-2024a
  set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; module load R/4.6.1-foss-2024a >/dev/null 2>&1
  set -o pipefail; Rscript --no-save '$r' 2>&1 | grep -v 'pool>'
" || rc=$?
rm -f "$r"
exit $rc
