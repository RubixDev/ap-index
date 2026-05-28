#!/bin/sh
set -e

docker build . -t ap-index
docker run --rm -v ./schema:/app/schema ap-index
