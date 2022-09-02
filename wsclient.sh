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

function cleanup() {
    stty cooked
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

echo "\
GET $path HTTP/1.1
Host: $host_and_port
User-Agent: $0
Connection: Upgrade
Upgrade: WebSocket
Sec-WebSocket-Key: $(echo asd asd asd asd | base64)
Sec-WebSocket-Version: 13
" >> $send

# Fifos can be opened for reading only once, so open it into a file descriptor
exec {recv_fd}<$recv

function fail() {
    echo "$0: $1" >&2
    exit 1
}

echo "Receiving status line"
read -r -u $recv_fd status_line || fail "receive error"
if [ "$status_line" != $'HTTP/1.1 101 Switching Protocols\r' ]; then
    fail "unexpected status line: $status_line"
fi

echo "Receiving headers"
while true; do
    read -r -u $recv_fd header_line || fail "receive error"
    if [ "$header_line" == $'\r' ]; then
        break
    fi
done

function receive_binary_frame() {
    # See "5.2. Base Framing Protocol" in RFC6455
    local received="$(head -c 2 <&$recv_fd | hexdump -v -e '1/1 "%02x"')"
    local byte1=0x${received:0:2}
    local byte2=0x${received:2:2}

    # bits 0-7: binary 10000100 = hex 0x82
    #   1           the last frame of a chunk of binary
    #   000         reserved bits
    #   0010        opcode 2 (bytes frame)
    if [ $byte1 != 0x82 ]; then
        fail "read frame: unexpected first byte: $byte1"
    fi

    # bits 8-15:
    #   0           masking not used
    #   xxxxxxx     payload length
    if (( byte2 >> 7 )); then
        fail "read frame: masking bit set, but masking is not supported"
    fi
    local payload_length=$(( byte2 ))

    if [ $payload_length == 126 ]; then
        # Next 16 bits are the payload length in big endian
        payload_length=$(( 0x$(head -c 2 <&$recv_fd | hexdump -v -e '1/1 "%02x"') ))
    elif [ $payload_length == 127 ]; then
        # Next 32 bits are the payload length in big endian
        payload_length=$(( 0x$(head -c 4 <&$recv_fd | hexdump -v -e '1/1 "%02x"') ))
    fi

    if [ $payload_length == 126 ] || [ $payload_length == 127 ]; then
        fail "not implemented: payload length $payload_length"
    fi

    head -c $payload_length <&$recv_fd
}

echo "Set terminal mode"
#stty raw

echo "Receiving websocket data"
#while true; do
#    receive_binary_frame
#done
