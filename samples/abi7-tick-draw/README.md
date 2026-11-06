# Inactive interface-7 tick/draw sample

This Linux x86-64 `no_std` sample is plugin-owned code for the published raw
contract. It is not an SDK facade, Battery conversion, UI, device image or
production CodeLoader activation. No hardware is needed.

Dependencies are explicit: registration/tick codecs from PR #152 at
`cdc9e2daa21a5d2c5176436f3b6094d2075d7cd7`, callback prefix readers from PR #151,
and the foreign H1/H2/H4 host proof in PR #153 at
`595f94510b45386ae114242366c57d4a03d70e4b`. This branch starts at that published
head. PR #153 reports completed supervised #2047, 13 native-runtime tests,
all-target clippy and eight quick-CI checks, plus independent Senior approval
of its four real-dlopen cases. Remote CI/merge status remains separate.

## Run

With the repository's pinned Rust and Python available, from the repository root:

```sh
export CARGO_TARGET_DIR="$PWD/target-abi7-tick-draw"
samples/abi7-tick-draw/verify.sh
```

The release artifact is `$CARGO_TARGET_DIR/release/deps/libpaperpaw_sample_abi7_tick_draw-<hash>.so`
(with the crate type given on the command line cargo uses a hashed name and does not list the
file among its artifacts; `verify.sh` removes older ones, builds, requires exactly one and prints
it as `artifact:`). `src/freestanding.rs` defines `rust_eh_personality` and `memcpy`, which the
linked library would otherwise import, and `verify.sh` fails if `nm -D --undefined-only`
lists any symbol, because a real `dlopen` of such a library fails. `check_artifact.py` also
disassembles `memcpy` to make sure it does not call itself (LLVM may turn a copy loop into a
call to `memcpy`) and copies buffers of up to 65,537 bytes through the library.

The standalone sample workspace does not change root manifests or SDK files.
`build.rs` invokes the canonical metadata generator and rejects unsupported
build targets. The release command also emits a Linux cdylib without startup
files. Metadata and callbacks are exported as PAPERPAW_METADATA/PAPERPAW_ENTRY.
No publishing, signing, automatic loading or hardware occurs.

`cargo test --lib` is the executable sample behavior proof: it calls the actual
sample make/event/draw/drop functions through generated C callback signatures.
The host uses the published neutral host table with only heap/clock/submission
bridges installed; registration bytes are decoded by the generated codec.
The test host is deliberately not a transaction/lease/loader implementation.
It does not claim to dlopen this sample or replace Firmware's H1/H2/H4 tests.
The cdylib is ready for a separately authorized reusable host runner once that
runner accepts plugin-owned sample artifacts; no duplicate loader fixture is
added here.

## Behavior

During make the sample submits two generated registrations: tick token1 and
draw token2. It uses the host allocator for one State, retaining the host table
and opaque context there, without writable plugin-global host state. It refuses
construction if registration or allocation fails, leaving output null.

Tick input is borrowed only through dispatch and decoded before state changes.
Each instance remembers tick time/count independently. Draw queries host time,
then records draw count/time on success. This is draw callback admission, not
painting: no Canvas or live Env service exists in this path yet. Host clock
errors leave the draw observation unchanged. Wrong tokens and malformed tick
records are refused. Drop returns the original block to the same host allocator.
The host owns admission, registration rollback, module/context leases and
exactly-once destruction; those properties come from the separately reviewed
foreign-runtime proof, not the sample's local behavior tests.

## Next concrete service handoff

To extend setting -> render -> event -> command without inventing an ABI:

1. Kernel/Firmware connect existing slot5 settings_read plus its reviewed typed
   codec to the owner's scoped live frame/context. The sample needs one Bool
   with absent/wrong-type outcomes, bounded copy and explicit error propagation.
   The standalone host currently refuses it; do not substitute a fake production
   setting or silently coerce missing to false.
2. Connect a borrowed Canvas frame to existing slot9 fill_rect for a minimal
   visible render, retaining raw validation and origin/clip/semantic behavior.
   Draw callback admission alone is not this render proof.
3. Tick/event dispatch exists. Slot15 still needs the reviewed generated command
   records, decoder and host-stamped authority for the chosen operation. Do not
   write guessed command bytes. Choose a concrete Notify/SetSetting operation
   with Kernel before extending this sample.

Battery's accepted fixtures (PR #148) remain the later full-package baseline.
This sample does not supply storage, usage, work, plugin metadata, rich i18n,
style, font, screen instances, menu/action registrations or device acceptance.
