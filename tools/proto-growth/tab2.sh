#!/usr/bin/env bash
B=~/fw-jobs/proto/build-fw.sh
$B ver-x4 x4,proto-ver
$B ver-x3-remote x3,remote,proto-ver
$B stress-x4 x4,proto-stress
$B stress-x3-remote x3,remote,proto-stress
