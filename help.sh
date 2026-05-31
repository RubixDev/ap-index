#!/bin/bash

while true; do
  printf "Display Name: "
  read -r display

  printf "Discord: "
  read -r discord
  tags=""
  if [[ "$discord" =~ '/1085716850370957462/' ]]; then
    tags='
tags = ["ad"]'
  fi

  printf "Download URL: "
  read -r url
  filename="${url##*/}"
  name="${filename%.apworld}"

  uri="${url#https://*/}"
  api_url="https://api.github.com/repos/${uri%/download/*}?per_page=100"
  echo fetching tags from "$api_url":
  gh_version="$(curl "$api_url" | jq '. | reverse | .[] | .tag_name' -r || echo 'failed to fetch')"
  if command -v wl-copy > /dev/null; then
    echo "$gh_version" | wl-copy
  fi
  echo "$gh_version"

  printf "Versions: "
  read -ra versions

  cat << EOF | tee -a index.toml

[[worlds]]
name = "$name"
display_name = "$display"$tags
discord = "$discord"
default_url = "$url"

  [worlds.versions]
EOF
  for version in "${versions[@]}"; do
    printf '  "%s" = ""\n' "$version" | tee -a index.toml
  done
  echo ----------------------------------------------
done
