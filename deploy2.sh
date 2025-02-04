#!/bin/bash
set -e -o pipefail

function info() {
    echo ""
    echo "========= $1 ========="
    echo ""
}

info "Check Git status"
# should be:
#   - on main branch
#   - working directory can contain untracked files (they are ignored)
#   - no added or not-added changes to files tracked with Git
LANG=C git status | grep 'On branch main'
[ "$(git status --porcelain | grep -v '^?? ')" == "" ]

info "Compile rust program"
rm -vf target/x86_64-unknown-linux-musl/release/catris
cargo build --release --target x86_64-unknown-linux-musl
ls -lh target/x86_64-unknown-linux-musl/release/catris

info "Copy files to /tmp/deploy"
ssh catris.net 'rm -rfv /tmp/deploy && mkdir -v /tmp/deploy'
scp -C target/x86_64-unknown-linux-musl/release/catris catris.service catris.net:/tmp/deploy/

echo ""
echo ""
ssh catris.net 'journalctl -u catris -n 10'
echo ""
echo "The above log output should show how many people are playing now."
echo "You can still abort the deploy, but then the following will not be updated:"
echo "  - The rust program"
echo "  - systemd service file"
echo "  - nginx configuration"
echo ""
read -p "Interrupt ongoing games and proceed with deploy? [y/N] " proceed
if [ "$proceed" != "y" ]; then
    echo "Abort."
    exit
fi

# These files are owned by root because changing them properly needs root permissions anyway
command='
   sudo systemctl stop catris
&& sudo cp /tmp/deploy/catris /home/catris/catris
&& sudo cp /tmp/deploy/catris.service /etc/systemd/system/catris.service
&& sudo systemctl daemon-reload
&& sudo systemctl start catris
&& sudo systemctl restart nginx
'
# Normalize whitespace
command="$(echo $command)"

info "Install files and restart services"
ssh -t catris.net "$command"

echo ""
echo "Done."
