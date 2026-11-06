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
  # A mirror that is slow or stalls must fail the step, not hold it for most
  # of an hour (the Azure mirror did, #876: 27 MB took five minutes, then the
  # next package ten). Retries and timeouts on every request, a ceiling on
  # each whole attempt (apt resumes partial files, so attempts add up), and
  # the .deb files kept between runs in $APT_DEB_CACHE when the action gives
  # one: a run that finds them downloads nothing.
  apt_options=(-o Acquire::Retries=5 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
  archives=${APT_ARCHIVES:-/var/cache/apt/archives}
  if [[ -n "${APT_DEB_CACHE:-}" ]]; then
    mkdir -p "$APT_DEB_CACHE"
    if compgen -G "$APT_DEB_CACHE/*.deb" >/dev/null; then
      sudo cp -n "$APT_DEB_CACHE"/*.deb "$archives"/
    fi
  fi
  attempt() { # attempt SECONDS COMMAND...: up to three tries, each cut off after SECONDS
    local seconds=$1 status=0 try
    shift
    for try in 1 2 3; do
      status=0
      sudo timeout "$seconds" "$@" || status=$?
      (( status == 0 )) && return 0
      echo "attempt $try of '$*' ended with $status" >&2
      (( try == 3 )) || sleep "${APT_RETRY_PAUSE:-2}"
    done
    return "$status"
  }
  attempt 120 apt-get "${apt_options[@]}" update
  attempt 240 apt-get "${apt_options[@]}" install -y "${missing[@]}"
  if [[ -n "${APT_DEB_CACHE:-}" ]]; then
    cp -n "$archives"/*.deb "$APT_DEB_CACHE"/ 2>/dev/null || true
  fi
fi

cmake --version
ninja --version
"$MBEDTLS_CLANG_PACKAGE" --version
{
  echo "CMAKE_GENERATOR=Ninja"
  echo "CLANG_PATH=$(command -v "$MBEDTLS_CLANG_PACKAGE")"
} >> "$GITHUB_ENV"
