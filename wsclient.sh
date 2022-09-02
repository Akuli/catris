#!/bin/bash
set -e -u -o pipefail

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

mkfifo $temp_dir/send
mkfifo $temp_dir/recv

if [ $scheme == wss ]; then
    openssl s_client -connect "$host:$port" -verify_return_error -quiet <$temp_dir/send >$temp_dir/recv &
else
    nc "$host" "$port" <$temp_dir/send >$temp_dir/recv &
fi

# Each end of a fifo can be opened only once, so open into file descriptors.
# For some reason the order of these lines matters
exec {send_fd}>$temp_dir/send
exec {recv_fd}<$temp_dir/recv

echo "\
GET $path HTTP/1.1
Host: $host_and_port
User-Agent: $0
Connection: Upgrade
Upgrade: WebSocket
Sec-WebSocket-Key: $(echo asd asd asd asd | base64)
Sec-WebSocket-Version: 13
" >&$send_fd

function fail() {
    echo "$0: $1" >&2
    exit 1
}

echo "Receiving status line"
LANG=C read -r -u $recv_fd status_line || fail "receive error"
if [ "$status_line" != $'HTTP/1.1 101 Switching Protocols\r' ]; then
    fail "unexpected status line: $status_line"
fi

echo "Receiving headers"
while true; do
    LANG=C read -r -u $recv_fd header_line || fail "receive error"
    if [ "$header_line" == $'\r' ]; then
        break
    fi
done

# --- from now on we send and receive binary, using hex to store it in variables ---
# Must be stored as hex because bash strings can't contain zero byte

function hex() {
    # hexdump is apparently not specified in posix standard, but od is:
    # https://pubs.opengroup.org/onlinepubs/9699919799/utilities/od.html
    od -A n -v -t x1 | tr -d ' '
}

function receive() {
    # Using head is unnecessary, even if converting the result to hex.
    # I tried hexdump's option to read n bytes, but then it still reads more than n bytes.
    head -c $1 <&$recv_fd
}

function unhex() {
    printf "$(grep -o '\S\S' | sed 's/^/\\x/g' | tr -d '\n')"
}

function send() {
    unhex >&$send_fd
}

function receive_binary_frame_to_stdout() {
    # See "5.2. Base Framing Protocol" in RFC6455

    # bits 0-7: binary 10000100 = hex 0x82
    #   1       the last frame of a chunk of binary
    #   000     reserved bits
    #   0010    opcode 2 (bytes frame)
    first_byte=$(( 0x$(receive 1 | hex) ))
    if (( first_byte != 0x82 )); then
        fail "read frame: bad first byte"
    fi

    # bits 8-15:
    #   0           masking not used
    #   xxxxxxx     payload length
    local payload_length=$(( 0x$(receive 1 | hex) ))
    if (( payload_length >> 7 )); then
        fail "read frame: masking bit set, but masking is not supported"
    fi

    if [ $payload_length == 126 ]; then
        # Next 16 bits are the payload length in big endian
        payload_length=$(( 0x$(receive 2 | hex) ))
    elif [ $payload_length == 127 ]; then
        # Next 32 bits are the payload length in big endian
        payload_length=$(( 0x$(receive 4 | hex) ))
    fi

    head -c $payload_length <&$recv_fd
}

function hex_xor() {
    printf '%x\n' $(( 0x$1 ^ 0x$2 ))
}

# need to go one byte at a time because "od" doesn't output anything before input EOF
function send_byte_from_stdin() {
    mask="$(head -c 4 /dev/urandom | hex)"
    # 0x82 = 10000010
    #   1       the last (and only) frame of this chunk of binary
    #   000     reserved bits
    #   0010    opcode 2 (bytes frame)
    #
    # 0x81 = 10000001
    #   1           masking enabled
    #   0000001     payload length is 1 byte
    #
    # I don't actually want to use masking, but the server errors if I don't.
    # I also tried setting the mask to all zeros, that errored too
    echo 82 81 $mask $(hex_xor ${mask:0:2} $(head -c 1 | hex)) | send
}

echo "Set terminal mode"
stty raw

echo "Send and receive data"
while receive_binary_frame_to_stdout; do :; done &

while true; do
    send_byte_from_stdin
done
