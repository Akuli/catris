#!/bin/bash
set -e -o pipefail
export LANG=C

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

if [ $scheme == ws ] && [ "$port" == "" ]; then port=80; fi
if [ $scheme == wss ] && [ "$port" == "" ]; then port=443; fi

temp_dir=$(mktemp -d)

function fail() {
    # Do not display multiple messages when quitting
    if mkdir $temp_dir/quit 2>/dev/null; then
        # terminal might be still in raw mode
        printf "\r\n\r\n%s: %s\r\n" "$0" "$1"
    fi
    return 1
}

function cleanup() {
    stty cooked
    rm -rf "$temp_dir"

    pids=$(jobs -p)
    if [ "$pids" != "" ]; then
        kill $pids
    fi
}
trap cleanup EXIT

mkfifo $temp_dir/send
mkfifo $temp_dir/recv

(
    if [ $scheme == wss ]; then
        openssl s_client -connect "$host:$port" -verify_return_error -quiet
    else
        nc "$host" "$port"
    fi
    fail "connection closed"
) <$temp_dir/send >$temp_dir/recv &

# Each end of a fifo can be opened only once, so open into file descriptors.
# For some reason the order of these lines matters
exec {send_fd}>$temp_dir/send
exec {recv_fd}<$temp_dir/recv

echo "Sending HTTP request"
echo "\
GET $path HTTP/1.1
Host: $host_and_port
User-Agent: $0
Connection: Upgrade
Upgrade: WebSocket
Sec-WebSocket-Key: $(echo asd asd asd asd | base64)
Sec-WebSocket-Version: 13
" >&$send_fd

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
    local result="$(head -c $1 <&$recv_fd | hex)"
    if [ "$result" == "" ]; then
        fail "connection closed"
    fi
    echo "$result"
}

function unhex() {
    # "echo abcd | unhex" does "printf '\xab\xcd'"
    printf "$(grep -o '\S\S' | sed 's/^/\\x/' | tr -d '\n')"
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
    local first_byte=$(( 0x$(receive 1) ))
    if (( first_byte != 0x82 )); then
        fail "read frame: bad first byte: $first_byte"
    fi

    # bits 8-15:
    #   0           masking not used
    #   xxxxxxx     payload length
    local payload_length=$(( 0x$(receive 1) ))
    if (( payload_length >> 7 )); then
        fail "read frame: masking bit set, but masking is not supported"
    fi

    if [ $payload_length == 126 ]; then
        # Next 16 bits are the payload length in big endian
        payload_length=$(( 0x$(receive 2) ))
    elif [ $payload_length == 127 ]; then
        # Next 32 bits are the payload length in big endian
        payload_length=$(( 0x$(receive 4) ))
    fi

    head -c $payload_length <&$recv_fd
}

# need to read stdin one byte at a time because "od" doesn't output anything before input EOF
function send_byte_from_stdin() {
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
    # So I use a hard-coded mask instead, lol
    local byte=0x$(head -c 1 | hex)
    local masked_byte=$(printf '%02x\n' $(( byte^0xFF )))
    echo 82 81 FFFFFFFF $masked_byte | send
}

echo "Set terminal mode"
stty raw

echo "Starting to send and receive data"
while true; do receive_binary_frame_to_stdout || fail "receive error"; done &
while true; do send_byte_from_stdin || fail "send error"; done </dev/stdin &

while ! [ -e $temp_dir/quit ]; do
    sleep 0.1
done
