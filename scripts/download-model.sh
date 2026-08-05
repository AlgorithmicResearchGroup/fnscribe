#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
model_dir="$project_dir/src-tauri/resources/models"
model_path="$model_dir/ggml-base.en-q5_1.bin"
model_url="https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en-q5_1.bin"
expected_sha256="4baf70dd0d7c4247ba2b81fafd9c01005ac77c2f9ef064e00dcf195d0e2fdd2f"

mkdir -p "$model_dir"

if [ -f "$model_path" ]; then
  actual_sha256=$(shasum -a 256 "$model_path" | awk '{print $1}')
  if [ "$actual_sha256" = "$expected_sha256" ]; then
    echo "Model is already downloaded: $model_path"
    exit 0
  fi
  echo "Existing model checksum is incorrect; downloading it again."
fi

temporary_path="$model_path.download"
trap 'rm -f "$temporary_path"' EXIT INT TERM

curl --fail --location --progress-bar "$model_url" --output "$temporary_path"
actual_sha256=$(shasum -a 256 "$temporary_path" | awk '{print $1}')

if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "Model checksum mismatch." >&2
  echo "Expected: $expected_sha256" >&2
  echo "Actual:   $actual_sha256" >&2
  exit 1
fi

mv "$temporary_path" "$model_path"
trap - EXIT INT TERM
echo "Downloaded model to $model_path"
