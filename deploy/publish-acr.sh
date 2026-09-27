#!/usr/bin/env bash
# Build/test a Linux image in Azure and publish it without local Docker or SQL credentials.
set -euo pipefail
registry="${1:?Usage: bash deploy/publish-acr.sh REGISTRY [TAG]}"
tag="${2:-$(date -u +%Y%m%dT%H%M%SZ)}"
if [[ ! "$registry" =~ ^[a-zA-Z0-9]{5,50}$ ]] || [[ ! "$tag" =~ ^[a-zA-Z0-9_][a-zA-Z0-9_.-]{0,127}$ ]]; then
    echo 'Invalid registry name or image tag.' >&2
    exit 2
fi
cd "$(dirname "$0")/.."
# This management request checks actual access, unlike a cached `az account show`.
login_server="$(az acr show --name "$registry" --query loginServer --output tsv --only-show-errors)"
az acr build --registry "$registry" --image "plant-journal:$tag" \
    --platform linux/amd64 --file Dockerfile .
# Executes the published runtime image without touching SQL, photos, or hardware.
az acr run --registry "$registry" \
    --cmd "$login_server/plant-journal:$tag --version" /dev/null
digest="$(az acr repository show --name "$registry" --image "plant-journal:$tag" \
    --query digest --output tsv --only-show-errors)"
printf 'Published and smoke-tested: %s/plant-journal:%s\nDigest: %s\n' "$login_server" "$tag" "$digest"
