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
  # each whole attempt (apt resumes partial files, so attempts add up; a
  # process that ignores the stop signal is killed after a grace period),
  # and the .deb files kept between runs in $APT_DEB_CACHE when the action
  # gives one: a run that finds them downloads nothing.
  apt_options=(-o Acquire::Retries=5 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
  archives=${APT_ARCHIVES:-/var/cache/apt/archives}
  attempt() { # attempt SECONDS COMMAND...: up to three tries, each cut off after SECONDS
    local seconds=$1 status=0 try
    shift
    for try in 1 2 3; do
      status=0
      sudo timeout --kill-after="${APT_KILL_GRACE:-10}" "$seconds" "$@" || status=$?
      (( status == 0 )) && return 0
      echo "attempt $try of '$*' ended with $status" >&2
      (( try == 3 )) || sleep "${APT_RETRY_PAUSE:-2}"
    done
    return "$status"
  }
  attempt "${APT_UPDATE_SECONDS:-90}" apt-get "${apt_options[@]}" update
  # What this install needs: the files' names, from the authenticated index.
  # (Nothing is downloaded for this: the package lists are here.)
  uris=$(attempt 30 apt-get "${apt_options[@]}" install --print-uris -qq -y "${missing[@]}")
  needed=()
  while IFS= read -r line; do
    [[ $line == "'"* ]] || continue
    read -r _ file _ <<<"$line"
    needed+=("$file")
  done <<<"$uris"
  if [[ -n "${APT_DEB_CACHE:-}" ]]; then
    mkdir -p "$APT_DEB_CACHE"
    for file in "${needed[@]}"; do
      [[ -f "$APT_DEB_CACHE/$file" ]] || continue
      # apt itself trusts a file already in its archive folder by its size;
      # a kept file is therefore given to apt only if its SHA-256 is that
      # of the authenticated package list (`apt-cache show`).
      package=${file%%_*}
      version=${file#*_}
      version=${version%_*}
      version=${version//%3a/:}
      expected=
      while read -r key value _; do
        [[ $key == "SHA256:" ]] && { expected=$value; break; }
      done < <(apt-cache show "$package=$version" 2>/dev/null)
      actual=$(sha256sum "$APT_DEB_CACHE/$file")
      if [[ -n "$expected" && "${actual%% *}" == "$expected" ]]; then
        sudo cp -n "$APT_DEB_CACHE/$file" "$archives"/
      else
        echo "kept $file does not match the package list: not used" >&2
        rm -f "$APT_DEB_CACHE/$file"
      fi
    done
  fi
  attempt "${APT_INSTALL_SECONDS:-180}" apt-get "${apt_options[@]}" install -y "${missing[@]}"
  if [[ -n "${APT_DEB_CACHE:-}" ]]; then
    for file in "${needed[@]}"; do
      if [[ -f "$archives/$file" ]]; then cp -n "$archives/$file" "$APT_DEB_CACHE"/; fi
    done
  fi
fi

cmake --version
ninja --version
"$MBEDTLS_CLANG_PACKAGE" --version
{
  echo "CMAKE_GENERATOR=Ninja"
  echo "CLANG_PATH=$(command -v "$MBEDTLS_CLANG_PACKAGE")"
} >> "$GITHUB_ENV"
