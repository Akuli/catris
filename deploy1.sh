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
#   - version number modified into Cargo.toml, not "git add"ed yet
#   - working directory can contain untracked files (they are ignored)
#   - no other changes
LANG=C git status | grep 'On branch main'
[ "$(git status --porcelain | grep -v '^?? ')" == " M Cargo.toml" ]

info "Commit, tag and push version based on Cargo.toml"
cargo build  # updates version number into Cargo.lock
git add Cargo.toml Cargo.lock
git commit -m "Bump version"
git show
git tag v$(grep ^version Cargo.toml | cut -d'"' -f2)
git push --tags origin main

info "Deploy web-ui"
scp $(git ls-files web-ui) catris.net:/var/www/html/

echo ""
echo "Done."
