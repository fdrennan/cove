#!/usr/bin/env bash
# release-project.sh <domain> <user> <admin:0|1> - give up a deleted project's
# name: the last step of Cove's Delete, after Archive has proven its history in
# S3 and removed its folder (Query::ProjectRelease; operator, 2026-10-26: "need
# option to delete or archive project").
#
# Run as root by `cove multiuser`. Archive alone leaves the domain registered
# and routed, so the router answers for code that is gone; this is the inverse
# of publishing, as git.ndexr.io's retire does it (registry deregister_domain +
# user_domains_remove), plus the vhost:
#
#   1. refuse while any src/domains/**/<domain> folder still exists;
#   2. as the platform account: the owner is <user> (or an admin asks),
#      deregister_domain, user_domains_remove;
#   3. move nginx/sites-enabled/<domain> to nginx/retired/ (kept, not deleted)
#      and reload nginx; a refused reload puts the vhost back.
set -euo pipefail
domain="${1:?domain}" user="${2:?user}" admin="${3:-0}"
[[ "$domain" =~ ^[a-z0-9][a-z0-9.-]+\.[a-z]+$ ]] || { echo "not a domain: $domain"; exit 2; }
[[ "$user" =~ ^[a-z_][a-z0-9_-]*$ ]] || { echo "not a user: $user"; exit 2; }
ROOT=/home/ubuntu/ndexr
for d in "$ROOT/src/domains/$domain" "$ROOT"/src/domains/*/"$domain"; do
  [[ -d "$d" ]] && { echo "$domain still has a folder ($d) - archive it first"; exit 2; }
done

r="$(runuser -u ubuntu -- mktemp -p /home/ubuntu/tmp release.XXXX.R)"
cat > "$r" <<RS
options(box.path = "$ROOT/src")
box::use(reg = r/domains/registry, ud = r/users/user_domains)
owner <- reg\$domain_owner("$domain")
who <- if (nzchar(owner)) reg\$username_for_email(owner) else NULL
if (!identical("$admin", "1") && nzchar(owner) && !identical(who, "$user")) {
  cat("$domain is not yours to delete\n"); quit(status = 3)
}
gone <- reg\$deregister_domain("$domain")
if (nzchar(owner)) ud\$user_domains_remove(owner, "$domain")
cat(if (is.null(gone)) "registry: already released" else "registry: released", "\n")
RS
chown ubuntu "$r"
rc=0
runuser -u ubuntu -- bash -c "
  cd '$ROOT'; set -a; . ./.env; set +a
  case \"\${POSTGRES_HOST:-}\" in postgres|pgbouncer) export POSTGRES_HOST=127.0.0.1 ;; esac
  [[ \"\${POSTGRES_DIRECT_HOST:-}\" == postgres ]] && export POSTGRES_DIRECT_HOST=127.0.0.1
  export NDEXR_HOST_LOCAL=1 LANG=C.UTF-8 R_LIBS=/opt/ndexr/eblib/x86_64-pc-linux-gnu/R-4.6.1-foss-2024a
  set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; module load R/4.6.1-foss-2024a >/dev/null 2>&1
  Rscript --no-save '$r' 2>&1 | grep -v -e 'pool>' -e 'OpenTelemetry'
" || rc=$?
rm -f "$r"
[[ $rc -eq 0 ]] || { echo "the registry was not changed"; exit $rc; }

vhost="$ROOT/nginx/sites-enabled/$domain"
if [[ -f "$vhost" ]]; then
  mkdir -p "$ROOT/nginx/retired"
  kept="$ROOT/nginx/retired/$domain.$(date -u +%Y%m%dT%H%M%SZ)"
  mv "$vhost" "$kept"
  chown ubuntu:ubuntu "$kept" "$ROOT/nginx/retired" 2>/dev/null || true
  unit=nginx
  systemctl cat ndexr-nginx >/dev/null 2>&1 && unit=ndexr-nginx
  # The host -> folder map every vhost reads (conf.d/domain_paths.conf),
  # regenerated as the platform's own reload does, so the name leaves it too.
  runuser -u ubuntu -- /home/ubuntu/.local/bin/ndexr nginx domain-paths >/dev/null 2>&1 || true
  if nginx -t >/dev/null 2>&1 && systemctl reload "$unit"; then
    echo "https://$domain/ is no longer served (vhost kept at nginx/retired/)"
  else
    mv "$kept" "$vhost"
    echo "nginx refused the reload - the vhost is back; the name is released in the registry"
    exit 1
  fi
else
  echo "site: already not served"
fi
echo "$domain is deleted"
