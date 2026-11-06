#!/usr/bin/env bash
# GitHub's Ubuntu image already supplies these commands. Install only what
# is absent, without pulling duplicate CMake/Ninja packages over apt mirrors.
set -euo pipefail
: "${MBEDTLS_CLANG_PACKAGE:?the release must name its mbedTLS clang}"
: "${GITHUB_ENV:?the action must provide its environment file}"

missing=()
for pair in "cmake:cmake" "ninja:ninja-build" "$MBEDTLS_CLANG_PACKAGE:$MBEDTLS_CLANG_PACKAGE"; do
  tool=${pair%%:*}
  command -v "$tool" >/dev/null || missing+=("${pair#*:}")
done
if (( ${#missing[@]} )); then
  # A mirror that stalls must fail the step (and the job's timeout end it),
  # not hold it for most of an hour: retries and timeouts on every request.
  apt_options=(-o Acquire::Retries=5 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
  sudo apt-get "${apt_options[@]}" update
  sudo apt-get "${apt_options[@]}" install -y "${missing[@]}"
fi

cmake --version
ninja --version
"$MBEDTLS_CLANG_PACKAGE" --version
{
  echo "CMAKE_GENERATOR=Ninja"
  echo "CLANG_PATH=$(command -v "$MBEDTLS_CLANG_PACKAGE")"
} >> "$GITHUB_ENV"
