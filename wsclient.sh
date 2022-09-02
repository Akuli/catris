#!/bin/bash
set -e -o pipefail

if [ $# != 1 ] || ! [[ "$1" =~ ^wss?://[^/] ]]; then
    echo "Usage: $0 <websocket-url>" >&2
    exit 2
fi
ws_or_wss_url="$1"

# Parse:
#   ws://hostname
#   ws://hostname/
#   ws://hostname/bla/bla/bla
#
# and these variations:
#   wss:// instead of ws://
#   hostname:123 instead of hostname (to specify a port number)
#
# Assumes hostname doesn't contain ":", so ipv6 addresses don't work.
scheme="$(echo "$ws_or_wss_url" | cut -d : -f 1)"
host_and_port="$(echo "$ws_or_wss_url" | cut -d / -f 3)"
path=/"$(echo "$ws_or_wss_url" | cut -d / -f 4-)"
host="$(echo "$host_and_port" | cut -d: -f1)"
port="$(echo "$host_and_port" | cut -d: -f2- -s)"

if [ $scheme == ws ] && [ "$port" == "" ]; then
    port=80
fi
if [ $scheme == wss ] && [ "$port" == "" ]; then
    port=443
fi

temp_dir=$(mktemp -d)

function cleanup()
{
    rm -rf "$temp_dir"

    pids=$(jobs -p)
    if [ "$pids" != "" ]; then
        kill $pids 2>/dev/null || true
    fi
}
trap cleanup EXIT

send=$temp_dir/send
recv=$temp_dir/recv
mkfifo $send
mkfifo $recv

if [ $scheme == wss ]; then
    openssl s_client -connect "$host:$port" -verify_return_error -quiet <$send >$recv &
else
    nc "$host" "$port" <$send >$recv &
fi

echo "GET $path HTTP/1.1" >> $send
echo "Host: $host_and_port" >> $send
echo "User-Agent: $0" >> $send
echo "Connection: Upgrade" >> $send
echo "Upgrade: websocket" >> $send
echo "Sec-WebSocket-Key: $(echo asd asd asd asd | base64)" >> $send
echo "Sec-WebSocket-Version: 13" >> $send
echo "" >> $send
cat $recv
