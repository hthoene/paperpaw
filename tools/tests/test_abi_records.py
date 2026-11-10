"""Registration/tick descriptors, generated coverage, and standalone wire vectors."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_records', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_records import rust_records, validate_records
from abi_declarations import rust_streams
from abi_i18n import rust_i18n
from abi_host_decode import rust_host_decode


RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: Registration = Registration::Event { event_tag: EVENT_TICK_TAG, token: 0x12345678 };
    const DRAW: Registration = Registration::Draw { token: u32::MAX };

    fn registrations(bytes: &[u8]) -> Result<std::vec::Vec<Registration>, i32> {
        registrations_decode(bytes).map(|batch| batch.iter().collect())
    }

    #[test]
    fn golden_vectors_and_roundtrips() {
        let expected = [2, 1, 9, 0, 3, 0, 0, 0, 0x78, 0x56, 0x34, 0x12,
                        2, 5, 0, 0xff, 0xff, 0xff, 0xff];
        let mut out = [0xa5; 32];
        assert_eq!(registrations_encode(&[EVENT, DRAW], &mut out), Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected);
        assert!(out[expected.len()..].iter().all(|&byte| byte == 0xa5));
        let batch = registrations_decode(&expected).unwrap();
        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
        assert_eq!(batch.iter().collect::<std::vec::Vec<_>>(), [EVENT, DRAW]);
        assert_eq!(registrations_decode(&[0]).unwrap().len(), 0);
        assert!(registrations_decode(&[0]).unwrap().is_empty());
        for count in 0..=REGISTRATION_MAX_RECORDS {
            let records = [EVENT; REGISTRATION_MAX_RECORDS];
            let mut out = [0; 128];
            let len = registrations_encode(&records[..count], &mut out).unwrap();
            assert_eq!(registrations(&out[..len]).unwrap(), records[..count]);
        }
        let tick = [1, 3, 9, 0, 8, 7, 6, 5, 4, 3, 2, 1];
        assert_eq!(tick_encode(0x0102030405060708, &mut out), Ok(tick.len()));
        assert_eq!(&out[..tick.len()], tick);
        assert_eq!(tick_decode(&tick), Ok(0x0102030405060708));
        for value in [0, 1, u64::MAX] {
            let len = tick_encode(value, &mut out).unwrap();
            assert_eq!(tick_decode(&out[..len]), Ok(value));
        }
    }

    #[test]
    fn output_is_unchanged_on_every_capacity_error() {
        let mut golden = [0; 128];
        for records in [&[][..], &[EVENT][..], &[EVENT, DRAW][..], &[DRAW; 8][..]] {
            let len = registrations_encode(records, &mut golden).unwrap();
            for cap in 0..len {
                let mut out = [0xa5; 128];
                assert_eq!(registrations_encode(records, &mut out[..cap]), Err(-3));
                assert_eq!(out, [0xa5; 128]);
            }
        }
        let mut out = [0xa5; 128];
        assert_eq!(registrations_encode(&[DRAW; 9], &mut out), Err(-3));
        assert_eq!(out, [0xa5; 128]);
        let len = tick_encode(1, &mut golden).unwrap();
        for cap in 0..len {
            assert_eq!(tick_encode(1, &mut out[..cap]), Err(-3));
            assert_eq!(out, [0xa5; 128]);
        }
    }

    #[test]
    fn truncated_and_malformed_frames() {
        let mut out = [0; 128];
        let len = registrations_encode(&[EVENT, DRAW], &mut out).unwrap();
        for cut in 0..len {
            assert_eq!(registrations(&out[..cut]), Err(-2));
        }
        let len = tick_encode(1, &mut out).unwrap();
        for cut in 0..len {
            assert_eq!(tick_decode(&out[..cut]), Err(-2));
        }
        let invalid = [
            &[][..], &[1], &[1, 2], &[1, 2, 0], // absent body bitmap
            &[1, 2, 1, 1], // bitmap longer than body
            &[1, 2, 5, 1, 0, 1, 2, 3], // required token incomplete
            &[1, 1, 5, 0, 1, 2, 3, 4], // missing event token
            &[1, 2, 5, 0, 1, 2, 3, 4, 0], // trailing outside record
            &[0, 0], // trailing outside empty batch
            &[1, 2, 0xff, 0xff, 0xff, 0xff, 0x0f], // enormous declared body
        ];
        for bytes in invalid {
            assert_eq!(registrations(bytes), Err(-2), "{bytes:?}");
        }
        assert_eq!(tick_decode(&[0]), Err(-2));
        assert_eq!(tick_decode(&[2]), Err(-2));
        let mut tick = std::vec![1, 3, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        tick.push(0);
        assert_eq!(tick_decode(&tick), Err(-2));
    }

    #[test]
    fn canonical_unsigned_u32_leb128_every_header_position() {
        let invalid = [
            &[0x80, 0][..], &[0x81, 0], &[0xff, 0xff, 0xff, 0xff, 0x10],
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0], &[0x80],
        ];
        for varint in invalid {
            for prefix in [&[][..], &[1][..], &[1, 2][..], &[1, 2, 12][..]] {
                let mut bytes = prefix.to_vec();
                bytes.extend_from_slice(varint);
                if prefix.len() == 3 {
                    bytes.resize(15, 0); // Complete record: malformed bitmap, not truncation.
                }
                assert_eq!(registrations(&bytes), Err(-2), "{bytes:?}");
            }
            for prefix in [&[][..], &[1][..], &[1, 3][..], &[1, 3, 12][..]] {
                let mut bytes = prefix.to_vec();
                bytes.extend_from_slice(varint);
                if prefix.len() == 3 {
                    bytes.resize(15, 0);
                }
                assert_eq!(tick_decode(&bytes), Err(-2), "{bytes:?}");
            }
        }
    }

    #[test]
    fn limits_unknown_tags_and_no_partial_batch() {
        assert_eq!(registrations(&[9]), Err(-3));
        assert_eq!(registrations(&[0xff, 0xff, 0xff, 0xff, 0x0f]), Err(-3));
        assert_eq!(registrations(&[1, 3, 0]), Err(-1));
        assert_eq!(tick_decode(&[1, 2, 0]), Err(-1));
        assert_eq!(registrations(&[2, 2, 5, 0, 1, 2, 3, 4, 99, 0]), Err(-1));
        let too_large = [0; REGISTRATION_MAX_BYTES + 1];
        assert_eq!(registrations(&too_large), Err(-3));
        assert_eq!(tick_decode(&too_large), Err(-3));
        // A maximal canonical u32 tag is supported framing, but an unknown record.
        assert_eq!(registrations(&[1, 0xff, 0xff, 0xff, 0xff, 0x0f, 0]), Err(-1));
    }

    fn extended(tag: u8, scalar: &[u8], total: usize) -> std::vec::Vec<u8> {
        // A two-byte canonical length covers the entire forward-compatible body.
        let body_len = total - 4;
        let mut bytes = std::vec![1, tag, (body_len as u8 & 0x7f) | 0x80,
                                 (body_len >> 7) as u8, 2, 0x80, 0xff];
        bytes.extend_from_slice(scalar);
        bytes.resize(total, 0xf1);
        bytes
    }

    #[test]
    fn forward_fields_and_extended_bitmaps_preserve_required_values() {
        let draw = [1, 2, 8, 2, 0x80, 0xff, 4, 3, 2, 1, 99];
        assert_eq!(registrations(&draw), Ok(std::vec![Registration::Draw { token: 0x01020304 }]));
        let event = [1, 1, 12, 1, 1, 3, 0, 0, 0, 7, 0, 0, 0, 42, 99];
        assert_eq!(registrations(&event), Ok(std::vec![Registration::Event {
            event_tag: 3, token: 7 }]));
        let tick = [1, 3, 11, 1, 1, 8, 7, 6, 5, 4, 3, 2, 1, 42];
        assert_eq!(tick_decode(&tick), Ok(0x0102030405060708));
        let max_draw = extended(2, &7_u32.to_le_bytes(), REGISTRATION_MAX_BYTES);
        assert_eq!(registrations(&max_draw), Ok(std::vec![Registration::Draw { token: 7 }]));
        let max_tick = extended(3, &7_u64.to_le_bytes(), REGISTRATION_MAX_BYTES);
        assert_eq!(tick_decode(&max_tick), Ok(7));
    }

    #[test]
    fn arbitrary_short_inputs_never_panic() {
        for first in 0..=255 {
            for second in 0..=255 {
                let bytes = [first, second];
                let _registration_result = registrations_decode(&bytes);
                let _tick_result = tick_decode(&bytes);
            }
        }
        let mut state = 0x12345678_u32;
        for len in 0..128 {
            let mut bytes = [0; 128];
            for byte in &mut bytes[..len] {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            let _registration_result = registrations_decode(&bytes[..len]);
            let _tick_result = tick_decode(&bytes[..len]);
        }
    }
}
'''


# Every generated copy of the records codec, byte for byte (#180).
COPIES = ['abi/generated/records_wire.rs',
          'crates/paperpaw-core/src/abi7/records_wire.rs',
          'crates/paperpaw-sdk/src/native/declare/records_wire.rs']


class RecordsTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def test_event_tags_stay_below_256(self):
        # `registration.event` names an event by tag and hosts keep per-plugin
        # event bitsets: every event record's tag stays below 256.
        events = [r for r in self.schema['records'] if r['name'].startswith('event.')]
        self.assertGreater(len(events), 40)
        self.assertEqual([(r['name'], r['id']) for r in events if r['id'] >= 256], [])

    def test_initial_descriptors_and_tags(self):
        ABI.validate(self.schema)
        self.assertEqual([(r['name'], r['id']) for r in self.schema['records']],
                         [('registration.event', 1), ('registration.draw', 2), ('event.tick', 3),
                          ('command.set_setting', 4), ('setting.group', 5),
                          ('setting.spec', 6), ('setting.extra', 7),
                          ('resource.hyphenation', 8), ('resource.font_face', 9),
                          ('resource.font_extension', 10), ('event.started', 32),
                          ('event.resume', 33), ('event.suspend', 34), ('event.stopping', 35),
                          ('event.battery_measured', 36), ('event.page_turned', 37),
                          ('service.usage', 48), ('service.plugin_usage', 49),
                          ('service.plugin', 50), ('service.work_note', 51),
                          ('service.end', 52), ('screen.spec', 11), ('action.spec', 12),
                          ('event.screen_shown', 38), ('event.action_run', 39),
                          ('input.key', 40), ('input.pointer', 41), ('input.gesture', 42),
                          ('input.action', 43), ('i18n.arg_str', 44), ('i18n.arg_int', 45),
                          ('i18n.count', 46), ('draw.frame', 192),
                          ('command.open_screen', 56), ('command.run_action', 57),
                          ('command.redraw', 58), ('command.close_screen', 59),
                          ('menu.entry', 13), ('icon.spec', 14), ('menu.need', 15),
                          ('menu.section', 16), ('menu.hide', 17), ('menu.describe', 18),
                          ('command.set_item_state', 60), ('stage.spec', 19),
                          ('registration.stage', 20), ('stage.call', 512), ('stage.reply', 513),
                          ('service.echo', 1024),
                          ('event.setting_changed', 64), ('event.book_opened', 65),
                          ('event.book_closed', 66), ('event.book_finished', 67),
                          ('event.book_moved', 68), ('event.book_removed', 69),
                          ('event.plugins_changed', 70), ('event.wifi_changed', 71),
                          ('event.power_changed', 72), ('event.status_changed', 73),
                          ('event.charge_changed', 74),
                          ('setup.step', 21), ('slot.spec', 22), ('slot.offer', 23),
                          ('key.bind', 24), ('setting.default', 25), ('setting.choice', 26),
                          ('preset.spec', 27), ('preset.value', 28),
                          ('event.registry_changed', 75), ('event.preview_changed', 76),
                          ('event.plugins_requested', 77), ('event.plugin_answered', 78),
                          ('service.reg_query', 1025), ('service.reg_key', 1026),
                          ('service.reg_setting', 1027), ('service.reg_setting_extra', 1028),
                          ('service.reg_choice', 1029), ('service.reg_text', 1030),
                          ('service.reg_group', 1031), ('service.reg_plugin', 1032),
                          ('service.reg_need', 1033), ('service.reg_item', 1034),
                          ('service.reg_slot', 1035), ('service.reg_menu_row', 1036),
                          ('service.reg_binding', 1037), ('service.reg_setup_step', 1038),
                          ('service.reg_value', 1039), ('service.reg_widget', 1040),
                          ('service.reg_status', 1059),
                          ('command.reset_setting', 1536), ('command.preview_setting', 1537),
                          ('command.preview_end', 1538), ('command.set_scope', 1539),
                          ('command.set_menu', 1540), ('command.bind_key', 1541),
                          ('command.bind_slot', 1542), ('command.set_plugin_enabled', 1543),
                          ('command.request_plugins', 1544), ('command.answer_plugin', 1545),
                          ('command.apply_preset', 1546),
                          ('input.spec', 29), ('help.hint', 30), ('widget.spec', 224),
                          ('widget.place', 225), ('status.item', 226), ('status.line', 227),
                          ('status.place', 228), ('event.screen_opened', 79),
                          ('event.screen_closed', 80), ('event.stack_changed', 81),
                          ('event.answered', 82), ('event.widget_made', 84),
                          ('event.widget_dropped', 85), ('event.widget_changed', 86),
                          ('event.screen_ready', 87), ('event.spoken', 89), ('input.widget', 193),
                          ('service.ui_screen', 1041), ('service.ui_request', 1042),
                          ('service.ui_question', 1043), ('service.ui_hint', 1044),
                          ('service.ui_widget_ref', 1045), ('service.ui_instance', 1046),
                          ('service.ui_widget_input', 1047), ('service.ui_target', 1048),
                          ('service.ui_ask', 1049), ('service.ui_text', 1050),
                          ('service.ui_text_at', 1051), ('service.ui_answer', 1052),
                          ('service.ui_widget_area', 1053), ('service.ui_size', 1054),
                          ('service.ui_status_ask', 1055), ('service.ui_status_value', 1056),
                          ('service.ui_param', 1057), ('service.ui_handle', 1058),
                          ('command.open', 1547), ('command.close', 1548),
                          ('command.back_to', 1549), ('command.set_key_layer', 1550),
                          ('command.show_hints', 1551), ('command.raise_input', 1552),
                          ('command.widget_changed', 1556), ('command.screen_ready', 1557),
                          ('command.notify', 1558), ('command.speak', 1561),
                          ('command.stop_speaking', 1562), ('command.screen_policy', 1563),
                          ('format.spec', 31), ('reader.spec', 229), ('service.doc_open', 1060),
                          ('service.doc_option', 1061), ('service.doc_state', 1062),
                          ('service.doc_ref', 1063), ('service.doc_meta', 1064),
                          ('service.doc_text', 1065), ('service.doc_text_at', 1066),
                          ('service.doc_page', 1067), ('service.doc_section', 1068),
                          ('service.doc_at', 1069), ('service.doc_toc', 1070), ('service.doc_index',
                          1071), ('service.doc_label', 1072), ('service.doc_xpointer', 1073),
                          ('service.doc_span', 1074), ('service.doc_link', 1075),
                          ('service.doc_href', 1076), ('service.doc_font', 1077),
                          ('service.doc_format', 1078), ('service.doc_path', 1079),
                          ('service.doc_reader', 1080), ('service.doc_progress', 1081),
                          ('service.doc_block', 1082), ('service.doc_text_part', 1090),
                          ('service.doc_columns', 1091), ('service.doc_run', 1083),
                          ('service.doc_map', 1084), ('service.doc_cell', 1085),
                          ('service.doc_ruby', 1086), ('service.doc_inline_lang', 1087),
                          ('service.doc_image', 1088), ('service.doc_image_ask', 1089),
                          ('command.image_prepare', 1559), ('command.image_release', 1560),
                          ('event.image_ready', 90), ('lookup.spec', 230), ('service.lookup_ask',
                          1092), ('service.lookup_entry', 1093), ('book_field.spec', 231),
                          ('book_badge.spec', 232), ('book_data.spec', 233),
                          ('event.library_changed', 91), ('service.lib_state', 1094),
                          ('service.lib_query', 1095), ('service.lib_entry', 1096),
                          ('service.lib_ref', 1097), ('service.lib_book', 1098),
                          ('service.lib_text_at', 1099), ('service.lib_text', 1100),
                          ('service.lib_facet', 1101), ('service.lib_cover', 1102),
                          ('service.lib_status', 1103), ('service.lib_paths', 1104),
                          ('service.lib_result', 1105), ('service.lib_move', 1106),
                          ('service.lib_import', 1107), ('service.lib_position', 1108),
                          ('service.lib_position_set', 1109), ('service.lib_page', 1110),
                          ('service.lib_annotation', 1111), ('service.lib_anchor', 1112),
                          ('service.lib_edit_head', 1113), ('service.lib_transaction', 1114),
                          ('service.lib_transaction_ref', 1115), ('service.lib_edit', 1116),
                          ('service.lib_edit_text', 1117), ('service.lib_committed', 1118),
                          ('service.lib_field', 1119), ('service.lib_field_set', 1120),
                          ('service.lib_provided', 1121), ('service.lib_key', 1122),
                          ('service.lib_receipt_ask', 1123), ('service.lib_receipt', 1124),
                          ('service.dev_device', 1125), ('service.dev_value', 1126), ('service.dev_firmware', 1127), ('service.dev_builtin', 1128), ('service.dev_native', 1129), ('service.dev_check', 1130), ('service.dev_memory', 1131), ('service.dev_wifi', 1132), ('service.dev_network', 1133), ('service.dev_bluetooth', 1134), ('service.dev_bonded', 1135), ('service.dev_interval', 1136), ('service.dev_power', 1137), ('service.dev_request', 1138), ('service.dev_install', 1139), ('service.dev_not_installed', 1140), ('service.dev_key', 1141), ('command.wifi_on', 1564), ('command.wifi_off', 1565), ('command.wifi_scan', 1566), ('command.wifi_join', 1567), ('command.wifi_leave', 1568), ('command.wifi_host', 1569), ('command.network_none', 1570), ('command.go_online', 1571), ('command.sleep', 1572), ('command.power_off', 1573), ('command.restart', 1574), ('command.wear', 1575), ('command.install_plugin', 1576), ('command.remove_plugin', 1577), ('command.install_firmware', 1578), ('command.firmware_restart', 1579), ('command.install_author', 1580), ('command.install_rotation', 1581), ('command.trust_revoked', 1582), ('command.check_file', 1592), ('command.bt_scan', 1583), ('command.bt_stop_scan', 1584), ('command.bt_pair', 1585), ('command.bt_connect', 1586), ('command.bt_disconnect', 1587), ('command.bt_forget', 1588), ('command.bt_give', 1589), ('command.bt_take_back', 1590), ('command.bt_keys', 1591), ('event.wifi_scanned', 92), ('event.wifi_done', 93), ('event.network_wanted', 94), ('event.network_idle', 95), ('event.firmware_changed', 96), ('event.install_done', 97), ('event.bluetooth', 98), ('service.fs_path', 1142), ('service.fs_list', 1143), ('service.fs_move', 1144), ('service.fs_entry', 1145), ('service.fs_space', 1146), ('service.fs_progress', 1147), ('event.http_done', 99), ('event.http_progress', 100), ('event.server', 101), ('event.served_request', 102), ('service.net_request', 1148), ('service.net_header', 1149), ('service.net_file', 1150), ('service.net_body', 1151), ('service.net_started', 1152), ('service.net_token', 1153), ('service.net_id', 1154), ('service.net_answer', 1155), ('service.net_read', 1156), ('service.net_bytes', 1157), ('service.net_serve', 1158), ('service.net_response', 1159), ('service.sec_name', 1160), ('service.sec_value', 1161), ('service.sec_list', 1162), ('service.sec_entry', 1163), ('service.sec_keep', 1164), ('service.sec_random', 1165), ('service.sec_bytes', 1166), ('event.work_due', 103), ('event.work_refused', 104), ('event.work_allowed', 105), ('service.work_spec', 1167), ('service.work_key', 1168), ('service.work_row', 1169), ('event.socket', 106), ('event.broadcast', 107), ('service.sock_connect', 1170), ('service.sock_pin', 1171), ('service.sock_started', 1172), ('service.sock_send', 1173), ('service.sock_read', 1174), ('service.sock_at', 1175), ('service.sock_bytes', 1176), ('service.sock_tls', 1177), ('service.sock_ref', 1178), ('service.sock_peer', 1179), ('service.sock_broadcast', 1180), ('service.sock_port', 1181), ('event.document_closed', 108), ('service.lib_field_page', 1182),
                          ('event.text_typed', 109), ('event.lookup_ready', 110),
                          ('command.lookup_ready', 1593),
                          ('command.set_hint', 1594),
                          ('event.document_changed', 88),
                          ('command.firmware_dual_boot', 1595)])
        changed = copy.deepcopy(self.schema)
        for record in changed['records']:
            if record['id'] <= 3:
                record['id'] = record['id'] % 3 + 1  # rotate within the initial range
        source = rust_records(changed)
        self.assertIn('EVENT_TICK_TAG: u32 = 1;', source)
        self.assertIn('REGISTRATION_EVENT_TAG: u32 = 2;', source)
        self.assertIn('REGISTRATION_DRAW_TAG: u32 = 3;', source)
        before = copy.deepcopy(self.schema)
        before['records'] = []
        del before['record_codec']
        del before['command_codec']
        self.assertEqual(rust_records(before), '')
        ABI.compatible(before, self.schema, 0)
        with self.assertRaisesRegex(ValueError, 'backdated'):
            ABI.compatible(before, self.schema, 1)
        later = copy.deepcopy(self.schema)
        later['minor'] = 2
        later['record_codec']['since'] = 2
        later['command_codec']['since'] = 2
        for record in later['records']:
            record['since'] = 2
            for field in record['fields']:
                field['since'] = 2
        ABI.compatible(before, later, 1)

    def test_known_shapes_are_validated(self):
        mutations = [('since', 2), ('fields', []), ('fields', None)]
        for key, value in mutations:
            candidate = copy.deepcopy(self.schema)
            candidate['minor'] = 2
            candidate['records'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for key, value in [('type', 'u64'), ('name', 'other'), ('since', True),
                           ('optional', True), ('ordinal', 0), ('default', 0),
                           ('max_bytes', True), ('max_bytes', 4)]:
            candidate = copy.deepcopy(self.schema)
            candidate['records'][0]['fields'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for index in range(3):
            candidate = copy.deepcopy(self.schema)
            candidate['records'].pop(index)
            with self.assertRaises(ValueError):
                ABI.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['records'][0]['fields'].reverse()
        with self.assertRaises(ValueError):
            ABI.validate(candidate)

    def test_count_descriptor_is_closed_and_frozen(self):
        for value in [None, {}, {'since': 1, 'max_records': 8, 'extra': 0},
                      {'since': True, 'max_records': 8}, {'since': 2, 'max_records': 8},
                      {'since': 1, 'max_records': True}, {'since': 1, 'max_records': 0},
                      {'since': 1, 'max_records': 9}]:
            candidate = copy.deepcopy(self.schema)
            candidate['record_codec'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                ABI.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['record_codec']['max_records'] = 4
        ABI.validate(candidate)
        self.assertIn('REGISTRATION_MAX_RECORDS: usize = 4;', rust_records(candidate))
        with self.assertRaisesRegex(ValueError, 'record codec contract'):
            ABI.compatible(self.schema, candidate)

    def test_lock_pins_standalone_emission(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['emitted_records'], rust_records(self.schema))
        original = ABI.rust_records
        with patch.object(ABI, 'rust_records',
                          lambda schema: original(schema).replace('return Err(-2)', 'return Err(-3)')):
            ABI.check_lock(ABI.lock_record(self.schema), self.schema)
            with self.assertRaisesRegex(ValueError, 'emitted records contract'):
                ABI.check_lock(trusted, self.schema)
        # The original raw module and settings emission do not acquire these types.
        self.assertNotIn('RegistrationBatch', ABI.rust_module(self.schema))

    def test_list_bounds_are_generated_from_the_schema(self):
        # #180: the choice limits come from abi.def, not literals in core or the SDK.
        spec = next(r for r in self.schema['records'] if r['name'] == 'setting.spec')
        items = next(f for f in spec['fields'] if f['name'] == 'choices')['list']
        source = rust_records(self.schema)
        self.assertIn(f"pub const SETTING_SPEC_CHOICES_MAX_ITEMS: usize = {items['max_items']};",
                      source)
        self.assertIn('pub const SETTING_SPEC_CHOICES_ITEM_MAX_BYTES: usize = '
                      f"{items['item_max_bytes']};", source)
        self.assertIn("pub const SETTING_SPEC_CHOICES_SEPARATOR: char = '\\n';", source)
        for path in ['crates/paperpaw-core/src/native_host/declarations.rs',
                     'crates/paperpaw-sdk/src/native/declare.rs']:
            text = (ROOT / path).read_text()
            self.assertIn('SETTING_SPEC_CHOICES_MAX_ITEMS', text, path)
            self.assertNotIn('CHOICES_MAX_VALUES: usize = 32', text, path)
        for bad in [dict(items, separator='ab'), dict(items, separator='\u00e9'),
                    dict(items, max_items=0), dict(items, item_max_bytes=2048),
                    dict(items, extra=1), {k: v for k, v in items.items() if k != 'separator'}]:
            candidate = copy.deepcopy(self.schema)
            field = next(f for f in next(r for r in candidate['records']
                                         if r['name'] == 'setting.spec')['fields']
                         if f['name'] == 'choices')
            field['list'] = bad
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                rust_records(candidate)
        candidate = copy.deepcopy(self.schema)
        field = next(f for f in next(r for r in candidate['records']
                                     if r['name'] == 'setting.spec')['fields']
                     if f['name'] == 'default')
        field['list'] = items
        with self.assertRaises(ValueError):
            rust_records(candidate)

    def test_counted_encoders_share_one_body(self):
        # Todo 46 (#228): the slice encoder delegates to the iterator one, so
        # both check caps and capacity in one place, before any write.
        source = rust_records(self.schema)
        for line in ['pub fn records_encode_counted_iter<\'r, I>(',
                     '    I: Iterator<Item = Record<\'r>> + Clone,',
                     '    records_encode_counted_iter(records.iter().copied(), max_records, max_bytes, out)',
                     '    let count = records.clone().take(max_records.saturating_add(1)).count();',
                     '    if written != count || offset != required {']:
            self.assertIn(line, source)
        self.assertEqual(source.count('write_varint(count as u32, out, &mut offset);'), 1)

    def test_standalone_copies_generated_and_checked(self):
        source = rust_records(self.schema) + rust_streams(self.schema) + rust_i18n(self.schema)
        served = ABI.with_services(self.schema, ROOT / 'abi/services')
        for path in COPIES:
            self.assertEqual((ROOT / path).read_text(), source + rust_host_decode(served))
        source += rust_host_decode(self.schema)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            (root / 'abi/abi.def').write_text(ABI.canonical(self.schema))
            ABI.run(['--root', str(root), '--write-lock'])
            for path in COPIES:
                generated = root / path
                self.assertEqual(generated.read_text(), source)
                generated.write_text('// stale')
                with self.assertRaisesRegex(ValueError, 'stale generated file'):
                    ABI.run(['--root', str(root), '--check'])
                ABI.run(['--root', str(root)])
            ABI.run(['--root', str(root), '--check'])

    def test_standalone_rust_no_std_and_vectors(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'records.rs'
            source.write_text('#![no_std]\n' + rust_records(self.schema) + rust_streams(self.schema))
            subprocess.run([rustc, '--edition=2024', '--crate-type=lib', '-Dwarnings',
                            str(source), '-o', str(folder / 'records.rlib')], check=True)
            source.write_text(rust_records(self.schema) + rust_streams(self.schema) + RUST_TESTS)
            binary = folder / 'records-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
