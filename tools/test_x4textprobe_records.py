"""Entirely invented offline fixtures, never captures or device measurements.

Record names and fields follow textprobe/mod.rs at commit
3e3b38ff73c83297d7767507696fd0dfa2be1ce8; numeric samples are synthetic.
"""

import copy
import unittest

from x4textprobe_records import ASCII_SHA256, Probe, compare


def synthetic(path="direct", baseline=100000, irq=20, retained=0):
    """Source-schema-complete fake data, with a configurable absolute baseline."""
    end, peak = baseline + retained, baseline + max(1024, retained)
    return [
        (f"PP:TEXTPROBE_BEGIN version=1 case=ascii path={path} cache=cold heap=clean "
         f"mode=raster fixture={ASCII_SHA256} font=synthetic-all repeats=1"),
        "PP:TEXTPROBE_STACK_SETUP requested=24704 charged=24712 size=24576",
        "PP:TEXTPROBE_SAMPLE repeat=0",
        (f"PP:TEXTPROBE_BEFORE used={baseline} free={262144-baseline} largest=120000 "
         "stack_left=20000 stack_size=40960 total=262144 boot_peak=150000"),
        (f"PP:TEXTPROBE_AFTER used={end} free={262144-end} largest=120000 "
         "stack_left=20000 stack_size=40960 total=262144 boot_peak=150000"),
        ("PP:TEXTPROBE_RESULT version=1 repeat=0 status=ok advance_bits=1065353216 "
         f"pixels={'0'*64} baseline_used={baseline} end_used={end} peak_used={peak} "
         "allocations=1 requested_bytes=1024 largest_request=1024 deallocations=1 "
         "freed_requested_bytes=1024 failed_allocations=0 elapsed_us=10 overflow=0 "
         f"scope_valid=1 min_free={262144-peak} retained_delta={retained}"),
        ("PP:TEXTPROBE_STACK repeat=0 method=paint-dedicated total=24576 unused=20000 "
         f"used=4576 guard_valid=1 irq_masked=1 irq_body_us={irq} admission_requested=98304 "
         "admission_align=4 admission_charged=98312 admission_released=1"),
        ("PP:TEXTPROBE_LIMITS stack_scope_supported=painted-depth-only "
         "request_alignment=unsupported semantic_hash=unsupported probe_resolution=64 "
         "heap_profile=clean warmups=0"),
        "PP:TEXTPROBE_END",
        "PP:END",
    ]


def parse(lines, path="direct"):
    probe = Probe(path)
    for line in lines:
        probe.feed(line)
    return probe.finish()


def changed(index, old, new, **kwargs):
    lines = synthetic(**kwargs)
    if old not in lines[index]:
        raise AssertionError(f"invalid test mutation: {old}")
    lines[index] = lines[index].replace(old, new)
    return lines


class ProbeTests(unittest.TestCase):
    def test_transport_end_requires_exact_wire_name_and_failure_is_permanent(self):
        for alias in ("PP:TEXTPROBE_TRANSPORT_END", "PP:TRANSPORT_END", "PP:END extra"):
            probe = Probe("direct")
            for line in synthetic()[:-1]:
                probe.feed(line)
            with self.subTest(alias=alias), self.assertRaises(ValueError):
                probe.feed(alias)
            self.assertIsNone(probe.result)
            with self.assertRaises(ValueError):
                probe.feed("PP:END")
            with self.assertRaises(ValueError):
                probe.finish()

    def test_complete_synthetic_protocol_for_all_paths(self):
        for path in ("control", "direct", "adapter"):
            with self.subTest(path=path):
                probe = Probe(path)
                self.assertFalse(probe.feed("ordinary firmware diagnostic"))
                lines = synthetic(path)
                for line in lines[:-1]:
                    self.assertFalse(probe.feed(line + "\r\n"))
                    self.assertIsNone(probe.result)
                self.assertTrue(probe.feed(lines[-1]))
                result = probe.finish()
                self.assertIs(result, probe.result)
                self.assertEqual(result["before"]["used"], 100000)
                self.assertEqual(result["after"]["total"], 262144)
                self.assertEqual(result["result"]["peak_used"], 101024)
                self.assertEqual(result["setup"]["charged"], 24712)
                self.assertEqual(result["stack"]["method"], "paint-dedicated")
                self.assertEqual(result["limits"]["request_alignment"], "unsupported")
                self.assertEqual(result["limits"]["semantic_hash"], "unsupported")
                self.assertEqual(result["advisories"], [])

    def test_truncation_at_every_record_never_completes(self):
        lines = synthetic()
        for stop in range(len(lines)):
            with self.subTest(stop=stop), self.assertRaises(ValueError):
                parse(lines[:stop])

    def test_missing_records_and_reordered_or_repeated_records_fail(self):
        lines = synthetic()
        variants = [lines[:i] + lines[i+1:] for i in range(len(lines))]
        variants += [lines[:i] + [lines[i]] + lines[i:] for i in range(len(lines))]
        for index in range(len(lines)-1):
            swap = list(lines)
            swap[index], swap[index+1] = swap[index+1], swap[index]
            variants.append(swap)
        for index, variant in enumerate(variants):
            with self.subTest(index=index), self.assertRaises(ValueError):
                parse(variant)

    def test_bad_schema_fields_and_invocation_dimensions(self):
        changes = [
            (0, "version=1", "version=2"), (0, "case=ascii", "case=hebrew"),
            (0, "path=direct", "path=adapter"), (0, "cache=cold", "cache=warm"),
            (0, "heap=clean", "heap=fragmented"), (0, "mode=raster", "mode=blind"),
            (0, "font=synthetic-all", "font=stored-fixture"), (0, "repeats=1", "repeats=10"),
            (0, ASCII_SHA256, "f"*64), (0, "version=1", "version=1 version=1"),
            (0, "version=1", "unknown=1"), (0, " version=1", ""),
            (0, "version=1", "version=1=2"), (1, "requested=24704", "requested=24703"),
            (1, "charged=24712", "charged=24703"), (1, "size=24576", "size=24575"),
            (2, "repeat=0", "repeat=1"), (5, "version=1", "version=2"),
            (5, "repeat=0", "repeat=1"), (6, "repeat=0", "repeat=1"),
        ]
        for index, old, new in changes:
            with self.subTest(old=old, new=new), self.assertRaises(ValueError):
                parse(changed(index, old, new))

    def test_malformed_numbers_and_hashes(self):
        numbers = ("-1", "1.5", "0x10", "nan", "+1", "4294967296", "1_000", "١")
        for value in numbers:
            with self.subTest(value=value), self.assertRaises(ValueError):
                parse(changed(5, "advance_bits=1065353216", f"advance_bits={value}"))
        for bits in (2139095040, 4286578688, 2143289344, 3212836864):
            with self.subTest(bits=bits), self.assertRaises(ValueError):
                parse(changed(5, "advance_bits=1065353216", f"advance_bits={bits}"))
        for pixel_hash in ("a"*63, "a"*65, "A"*64, "g"*64):
            with self.subTest(pixel_hash=pixel_hash), self.assertRaises(ValueError):
                parse(changed(5, "pixels=" + "0"*64, "pixels=" + pixel_hash))
        for old, new in (("allocations=1", "allocations=-1"),
                         ("elapsed_us=10", "elapsed_us=18446744073709551616"),
                         ("retained_delta=0", "retained_delta=9223372036854775808")):
            with self.subTest(new=new), self.assertRaises(ValueError):
                parse(changed(5, old, new))

    def test_failed_scope_or_unsupported_record_is_terminal(self):
        changes = [(5, "scope_valid=1", "scope_valid=0"),
                   (5, "status=ok", "status=draw-error"),
                   (5, "failed_allocations=0", "failed_allocations=1"),
                   (5, "overflow=0", "overflow=1"),
                   (6, "guard_valid=1", "guard_valid=0")]
        for index, old, new in changes:
            with self.subTest(new=new), self.assertRaises(ValueError):
                parse(changed(index, old, new))
        for record in ("PP:TEXTPROBE_ERROR reason=headroom",
                       "PP:TEXTPROBE_UNSUPPORTED case=ascii mode=blind reason=unqualified",
                       "PP:TEXTPROBE_WARMUP index=0 irq_body_us=20 stack_used=100 guard_valid=1"):
            probe = Probe("direct")
            with self.subTest(record=record):
                with self.assertRaises(ValueError):
                    probe.feed(record)
                with self.assertRaises(ValueError):
                    probe.feed(synthetic()[0])
                with self.assertRaises(ValueError):
                    probe.finish()

    def test_stack_admission_and_limits_are_exact(self):
        changes = [
            (6, "method=paint-dedicated", "method=minimum-sp"),
            (6, "total=24576", "total=24575"), (6, "used=4576", "used=4577"),
            (6, "unused=20000 used=4576", "unused=10239 used=14337"),
            (6, "irq_masked=1", "irq_masked=0"),
            (6, "admission_requested=98304", "admission_requested=98303"),
            (6, "admission_align=4", "admission_align=16"),
            (6, "admission_released=1", "admission_released=0"),
            (6, "admission_charged=98312", "admission_charged=98303"),
            (6, "admission_charged=98312", "admission_charged=200000"),
            (7, "stack_scope_supported=painted-depth-only", "stack_scope_supported=exact-sp"),
            (7, "request_alignment=unsupported", "request_alignment=4"),
            (7, "semantic_hash=unsupported", "semantic_hash=0"),
            (7, "probe_resolution=64", "probe_resolution=1"),
            (7, "heap_profile=clean", "heap_profile=synthetic-holes-v1"),
            (7, "warmups=0", "warmups=2"), (7, "warmups=0", "warmups=0 future=1"),
        ]
        for index, old, new in changes:
            with self.subTest(new=new), self.assertRaises(ValueError):
                parse(changed(index, old, new))
        for index in (3, 4):
            with self.subTest(index=index), self.assertRaises(ValueError):
                parse(changed(index, "stack_left=20000", "stack_left=10239"))

    def test_irq_thresholds_and_elapsed_consistency(self):
        for irq in (1000, 1001, 10000):
            with self.subTest(irq=irq):
                self.assertEqual(bool(parse(synthetic(irq=irq))["advisories"]), irq > 1000)
        with self.assertRaisesRegex(ValueError, "STOP"):
            parse(synthetic(irq=10001))
        with self.assertRaisesRegex(ValueError, "elapsed"):
            parse(synthetic(irq=9))

    def test_heap_and_traffic_consistency(self):
        changes = [
            (3, "total=262144", "total=262145"),
            (4, "free=162144", "free=162143"), (3, "largest=120000", "largest=200000"),
            (3, "stack_left=20000", "stack_left=40961"),
            (4, "stack_size=40960", "stack_size=40961"),
            (3, "boot_peak=150000", "boot_peak=99999"),
            (4, "boot_peak=150000", "boot_peak=149999"),
            (5, "baseline_used=100000", "baseline_used=99999"),
            (5, "end_used=100000", "end_used=99999"),
            (5, "peak_used=101024", "peak_used=99999"),
            (5, "peak_used=101024", "peak_used=262145"),
            (5, "min_free=161120", "min_free=161119"),
            (5, "retained_delta=0", "retained_delta=1"),
            (5, "largest_request=1024", "largest_request=1025"),
            (5, "allocations=1", "allocations=0"),
            (5, "allocations=1", "allocations=2000"),
            (5, "deallocations=1", "deallocations=0"),
            (5, "deallocations=1", "deallocations=2000"),
        ]
        for index, old, new in changes:
            with self.subTest(new=new), self.assertRaises(ValueError):
                parse(changed(index, old, new))
        self.assertEqual(parse(synthetic(retained=-128))["result"]["retained_delta"], -128)

    def test_heap_threshold_boundary_uses_absolute_heap_with_stack(self):
        lines = synthetic()
        lines[4] = lines[4].replace("boot_peak=150000", "boot_peak=222822")
        lines[5] = lines[5].replace("peak_used=101024", "peak_used=222822")
        lines[5] = lines[5].replace("min_free=161120", "min_free=39322")
        self.assertEqual(parse(lines)["result"]["min_free"], 39322)
        lines[4] = lines[4].replace("222822", "222823")
        lines[5] = lines[5].replace("222822", "222823").replace("39322", "39321")
        with self.assertRaises(ValueError):
            parse(lines)
        with self.assertRaises(ValueError):
            parse(changed(1, "charged=24712", "charged=100001"))

    def test_actual_heap_total_is_preserved_and_each_heap_limit_is_independent(self):
        lines = synthetic()
        for index in (3, 4):
            lines[index] = lines[index].replace("free=162144", "free=170000")
            lines[index] = lines[index].replace("total=262144", "total=270000")
        lines[5] = lines[5].replace("min_free=161120", "min_free=168976")
        self.assertEqual(parse(lines)["after"]["total"], 270000)
        high_peak = list(lines)
        high_peak[4] = high_peak[4].replace("boot_peak=150000", "boot_peak=222823")
        high_peak[5] = high_peak[5].replace("peak_used=101024", "peak_used=222823")
        high_peak[5] = high_peak[5].replace("min_free=168976", "min_free=47177")
        with self.assertRaisesRegex(ValueError, "scope heap headroom"):
            parse(high_peak)
        low_free = synthetic()
        for index in (3, 4):
            low_free[index] = low_free[index].replace("free=162144", "free=40345")
            low_free[index] = low_free[index].replace("largest=120000", "largest=40000")
            low_free[index] = low_free[index].replace("total=262144", "total=140345")
            low_free[index] = low_free[index].replace("boot_peak=150000", "boot_peak=110000")
        low_free[5] = low_free[5].replace("min_free=161120", "min_free=39321")
        with self.assertRaisesRegex(ValueError, "scope heap headroom"):
            parse(low_free)

    def test_zero_allocation_sample_and_stack_boundary_are_valid(self):
        lines = synthetic("control")
        lines[5] = lines[5].replace("advance_bits=1065353216", "advance_bits=0")
        lines[5] = lines[5].replace("peak_used=101024", "peak_used=100000")
        lines[5] = lines[5].replace("min_free=161120", "min_free=162144")
        lines[5] = lines[5].replace("allocations=1", "allocations=0")
        lines[5] = lines[5].replace("requested_bytes=1024", "requested_bytes=0")
        lines[5] = lines[5].replace("largest_request=1024", "largest_request=0")
        lines[6] = lines[6].replace("unused=20000 used=4576", "unused=10240 used=14336")
        for index in (3, 4):
            lines[index] = lines[index].replace("stack_left=20000", "stack_left=10240")
        result = parse(lines, "control")
        self.assertEqual(result["result"]["allocations"], 0)
        self.assertEqual(result["stack"]["unused"], 10240)

    def test_non_protocol_logs_and_fatal_indicators(self):
        for log in ("rst:0x1", "ESP-ROM:esp32", "panic at location", "Guru Meditation Error",
                    "Rebooting...", "Brownout detector", "watchdog", "connection lost",
                    "serial port closed", "timed out", "EOF", "reset detected"):
            with self.subTest(log=log), self.assertRaises(ValueError):
                parse([synthetic()[0], log] + synthetic()[1:])
        for line in ("prefix PP:TEXTPROBE_END", "PP:UNKNOWN", "x"*4097,
                     "ordinary log\nPP:END", "PP:TEXTPROBE_BEGIN\u00a0version=1"):
            with self.subTest(line=line[:40]), self.assertRaises(ValueError):
                Probe("direct").feed(line)


class CompareTests(unittest.TestCase):
    def test_parity_uses_peak_extra_and_allocation_traffic(self):
        direct = parse(synthetic())
        adapter = parse(synthetic("adapter", baseline=101000), "adapter")
        self.assertEqual(compare(direct, adapter)["peak_extra"], 1024)
        for field in ("advance_bits", "pixels", "allocations", "requested_bytes", "largest_request",
                      "deallocations", "freed_requested_bytes", "retained_delta", "peak_used"):
            changed_adapter = copy.deepcopy(adapter)
            value = changed_adapter["result"][field]
            changed_adapter["result"][field] = "f"*64 if field == "pixels" else value + 1
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "parity mismatch"):
                compare(direct, changed_adapter)

    def test_incomplete_control_wrong_order_and_missing_fields_rejected(self):
        direct, adapter = Probe("direct"), Probe("adapter")
        with self.assertRaises(ValueError):
            compare(direct, adapter)
        for probe in (direct, adapter):
            for line in synthetic(probe.path):
                probe.feed(line)
        self.assertEqual(compare(direct, adapter)["advance_bits"], 1065353216)
        control = parse(synthetic("control"), "control")
        for left, right in ((control, adapter.result), (adapter.result, direct.result),
                            ({"path": "direct"}, adapter.result)):
            with self.subTest(left=left["path"]), self.assertRaises(ValueError):
                compare(left, right)


if __name__ == "__main__":
    unittest.main()
