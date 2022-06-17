#!/bin/bash

# This script backs up the high scores file from production with ssh.
# You can run this as a cron job, for example.

set -e

if [ $# -ne 1 ]; then
    echo "Usage: $0 <dest-dir>" >&2
    exit 2
fi

mkdir -vp "$1"
cd "$1"

delete_older_than=$(date +'%Y-%m-%d' -d '30 days ago')
compress_older_than=$(date +'%Y-%m-%d' -d '3 days ago')

function timestamp_of {
    # strip hours, minutes and file extension
    date -d $(echo $1 | cut -d_ -f1) +%s
}

shopt -s nullglob
for file in *.txt *.txt.gz; do
    if [ $(timestamp_of $file) -lt $(timestamp_of $delete_older_than) ]; then
        rm -v $file
    fi
done
for file in *.txt; do
    if [ $(timestamp_of $file) -lt $(timestamp_of $compress_older_than) ]; then
        gzip -v $file
    fi
done

filename=$(date +'%Y-%m-%d_%H-%M-%S.txt')
if [ -e $filename ]; then
    echo "Error: refusing to overwrite $filename" >&2
    exit 1
fi
scp catris:/home/catris/catris_high_scores.txt $filename
