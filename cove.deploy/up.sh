#!/usr/bin/env bash
# Start a fresh EC2 box with Cove on it and print the sign-in URL.
#
#   cove.deploy/up.sh [name]          # name defaults to cove-test-<UTC time>
#
# What it does: makes a security group (22 from your IP, 8788 from anywhere),
# launches the latest Ubuntu 24.04 with your ~/.ssh/id_ed25519.pub let in,
# installs Cove with the public one-liner (curl -fsSL https://ndexr.io/install.sh | sh),
# runs `cove serve` on :8788 as a systemd unit behind a fresh token, waits
# until sign-in works, and writes everything down.kill needs to
# cove.deploy/state/<name>.env.
#
# Plain HTTP: the token crosses the wire unencrypted. For throwaway tests.
#
# Overrides (env): AWS_REGION (us-east-2), INSTANCE_TYPE (t3.medium),
# ARCH (amd64|arm64, amd64), DISK_GB (30), KEY_NAME (default), PUBKEY (~/.ssh/id_ed25519.pub),
# MOUNT_STACK (no|yes, no: the installer's "mount the ndexr stack?" question,
# answered here because cloud-init has no terminal to ask it on).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
name="${1:-cove-test-$(date -u +%Y%m%d-%H%M%S)}"
export AWS_REGION="${AWS_REGION:-us-east-2}"
type="${INSTANCE_TYPE:-t3.medium}"
arch="${ARCH:-amd64}"
disk="${DISK_GB:-30}"
key_name="${KEY_NAME:-default}"
pubkey_file="${PUBKEY:-$HOME/.ssh/id_ed25519.pub}"
mount_stack="${MOUNT_STACK:-no}"
port=8788
state="$here/state/$name.env"

[[ -e "$state" ]] && { echo "✗ $state exists - '$name' is already up (down.sh $name first)" >&2; exit 1; }
[[ -r "$pubkey_file" ]] || { echo "✗ no public key at $pubkey_file (set PUBKEY)" >&2; exit 1; }
mkdir -p "$here/state"

say() { printf '  › %s\n' "$*"; }

token="$(head -c 32 /dev/urandom | base64 | tr -d '/+=\n')"
pub="$(cat "$pubkey_file")"
myip="$(curl -fsS https://checkip.amazonaws.com | tr -d '[:space:]')"

say "finding the latest Ubuntu 24.04 ($arch) AMI"
ami="$(aws ec2 describe-images --owners 099720109477 \
  --filters "Name=name,Values=ubuntu/images/hvm-ssd-gp3/ubuntu-noble-24.04-$arch-server-*" \
  --query 'sort_by(Images,&CreationDate)[-1].ImageId' --output text)"

say "security group $name (22 from $myip, $port from anywhere)"
sg="$(aws ec2 create-security-group --group-name "$name" --description "cove test box $name" \
  --tag-specifications "ResourceType=security-group,Tags=[{Key=Name,Value=$name},{Key=cove-deploy,Value=$name}]" \
  --query GroupId --output text)"
# Written now, so down.sh can clean up even if a later step fails.
printf 'NAME=%s\nAWS_REGION=%s\nSG=%s\n' "$name" "$AWS_REGION" "$sg" > "$state"
aws ec2 authorize-security-group-ingress --group-id "$sg" --protocol tcp --port 22 --cidr "$myip/32" >/dev/null
aws ec2 authorize-security-group-ingress --group-id "$sg" --protocol tcp --port "$port" --cidr 0.0.0.0/0 >/dev/null

userdata="$(mktemp)"; trap 'rm -f "$userdata"' EXIT
cat > "$userdata" <<EOF
#!/bin/bash
set -eux
echo '$pub' >> /home/ubuntu/.ssh/authorized_keys
export DEBIAN_FRONTEND=noninteractive
apt-get update -y
apt-get install -y curl libwebkit2gtk-4.1-0
sudo -iu ubuntu bash -c 'curl -fsSL https://ndexr.io/install.sh | COVE_MOUNT_STACK=$mount_stack sh' </dev/null
install -d -m 700 -o ubuntu -g ubuntu /home/ubuntu/.config/ndexr
printf '%s\n' '$token' > /home/ubuntu/.config/ndexr/cove-token
chown ubuntu:ubuntu /home/ubuntu/.config/ndexr/cove-token
chmod 600 /home/ubuntu/.config/ndexr/cove-token
cat > /etc/systemd/system/cove.service <<'UNIT'
[Unit]
Description=cove serve
After=network-online.target
Wants=network-online.target
[Service]
User=ubuntu
WorkingDirectory=/home/ubuntu
ExecStart=/home/ubuntu/.local/bin/cove serve --listen 0.0.0.0:$port --web-dir /home/ubuntu/.local/share/cove/web --token-file /home/ubuntu/.config/ndexr/cove-token
Restart=always
[Install]
WantedBy=multi-user.target
UNIT
systemctl daemon-reload
systemctl enable --now cove
EOF

say "launching $type"
id="$(aws ec2 run-instances --image-id "$ami" --instance-type "$type" --key-name "$key_name" \
  --security-group-ids "$sg" \
  --block-device-mappings "DeviceName=/dev/sda1,Ebs={VolumeSize=$disk,VolumeType=gp3,DeleteOnTermination=true}" \
  --user-data "file://$userdata" \
  --tag-specifications "ResourceType=instance,Tags=[{Key=Name,Value=$name},{Key=cove-deploy,Value=$name}]" \
  --query 'Instances[0].InstanceId' --output text)"
echo "INSTANCE=$id" >> "$state"

aws ec2 wait instance-running --instance-ids "$id"
ip="$(aws ec2 describe-instances --instance-ids "$id" --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)"
url="http://$ip:$port/login?token=$token"
printf 'IP=%s\nTOKEN=%s\nURL=%s\n' "$ip" "$token" "$url" >> "$state"
chmod 600 "$state"

say "$id at $ip - waiting for Cove to install and answer (a few minutes)"
for _ in $(seq 1 60); do
  code="$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 "$url" || true)"
  [[ "$code" == 302 ]] && break
  sleep 10
done
if [[ "$code" != 302 ]]; then
  echo "✗ Cove did not answer in 10 minutes. Look:  ssh ubuntu@$ip 'cloud-init status; sudo tail -50 /var/log/cloud-init-output.log'" >&2
  echo "  tear down:  $here/down.sh $name" >&2
  exit 1
fi

printf '\n  ✓ %s is up\n\n' "$name"
printf '    URL   %s\n' "$url"
printf '    SSH   ssh ubuntu@%s\n' "$ip"
printf '    Kill  %s/down.sh %s\n\n' "$here" "$name"
