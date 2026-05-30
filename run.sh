#!/bin/sh
set -e

docker build . -t ap-index
docker run --rm -v ./schema:/app/schema -v ./worlds:/app/custom_worlds ap-index
