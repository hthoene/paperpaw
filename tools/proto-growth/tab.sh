#!/usr/bin/env bash
B=~/fw-jobs/proto/build-fw.sh
for v in all big; do
$B tab-$v-x4 x4,proto-$v
$B tab-$v-x4-dev x4,remote,waveform-lab,proto-$v
$B tab-$v-x3 x3,proto-$v
$B tab-$v-x3-remote x3,remote,proto-$v
done
