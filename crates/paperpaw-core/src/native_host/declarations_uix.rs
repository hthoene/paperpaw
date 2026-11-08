//! Allocation-free widget and status syntax checks before registry admission.
//! Reference resolution and token ownership belong to the admission transaction.
use super::{
    declarations::{full_id, relative},
    registrations::{
        Record, StatusItem, StatusLine, StatusPlace, WIDGET_PLACE_LIST_KEYS_MAX_ITEMS,
        WIDGET_PLACE_LIST_KEYS_SEPARATOR, WidgetPlace, WidgetSpec,
    },
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_UNSUPPORTED};

const PRIVATE: u32 = 1 << 1;
const LIVE: u32 = 1;
const CELL_MASK: u32 = 0xffff;

/// Checks only local syntax. Tokens are opaque, including zero; admission
/// checks their callback registrations and references against the retained owner.
pub(crate) fn validate(plugin: &str, record: &Record<'_>) -> Result<(), i32> {
    match record {
        Record::WidgetSpec(spec) => widget(plugin, spec),
        Record::WidgetPlace(place) => widget_place(plugin, place),
        Record::StatusItem(item) => status_item(plugin, item),
        Record::StatusLine(line) => status_line(plugin, line),
        Record::StatusPlace(place) => status_place(plugin, place),
        _ => Err(ERR_UNSUPPORTED),
    }
}

fn named(plugin: &str, name: &str, title: &str) -> Result<(), i32> {
    relative(plugin, name)?;
    if title.is_empty() {
        Err(ERR_INVALID)
    } else {
        Ok(())
    }
}

fn qualified(id: &str) -> Result<(), i32> {
    full_id(id)?;
    if id.contains('.') {
        Ok(())
    } else {
        Err(ERR_INVALID)
    }
}

fn widget(plugin: &str, spec: &WidgetSpec<'_>) -> Result<(), i32> {
    named(plugin, spec.name, spec.title)?;
    // Categories may be catalog keys, so their text is not an identifier.
    if spec.category.is_empty()
        || spec.cells & !CELL_MASK != 0
        || spec.flags & !(LIVE | PRIVATE) != 0
    {
        return Err(ERR_INVALID);
    }
    if !spec.previews.is_empty() {
        qualified(spec.previews)?;
    }
    Ok(())
}

fn widget_place(plugin: &str, place: &WidgetPlace<'_>) -> Result<(), i32> {
    named(plugin, place.name, place.title)?;
    if place.layout > 1 || place.shown_if_key.is_empty() != place.shown_if_value.is_empty() {
        return Err(ERR_INVALID);
    }
    if !place.shown_if_key.is_empty() {
        qualified(place.shown_if_key)?;
    }
    if !place.list_keys.is_empty() {
        let mut count = 0;
        for key in place.list_keys.split(WIDGET_PLACE_LIST_KEYS_SEPARATOR) {
            count += 1;
            if count > WIDGET_PLACE_LIST_KEYS_MAX_ITEMS {
                return Err(ERR_INVALID);
            }
            qualified(key)?;
        }
    }
    Ok(())
}

fn status_item(plugin: &str, item: &StatusItem<'_>) -> Result<(), i32> {
    named(plugin, item.name, item.title)?;
    if item.priority > u32::from(u8::MAX) || item.flags & !PRIVATE != 0 {
        return Err(ERR_INVALID);
    }
    if !item.tap.is_empty() {
        relative(plugin, item.tap)?;
    }
    Ok(())
}

fn status_line(plugin: &str, line: &StatusLine<'_>) -> Result<(), i32> {
    named(plugin, line.name, line.title)?;
    if line.flags & !PRIVATE != 0 {
        Err(ERR_INVALID)
    } else {
        Ok(())
    }
}

fn status_place(plugin: &str, place: &StatusPlace<'_>) -> Result<(), i32> {
    named(plugin, place.name, place.title)?;
    for key in [place.left, place.center, place.right] {
        qualified(key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget() -> WidgetSpec<'static> {
        WidgetSpec {
            name: "clock",
            title: "Clock",
            category: "widget.category.clock",
            previews: "",
            cells: 0,
            token: 0,
            flags: 0,
        }
    }

    fn place() -> WidgetPlace<'static> {
        WidgetPlace {
            name: "home",
            title: "Home",
            list_keys: "home.widgets\nhome.widgets-two",
            layout: 0,
            shown_if_key: "",
            shown_if_value: "",
        }
    }

    fn item() -> StatusItem<'static> {
        StatusItem {
            name: "battery",
            title: "Battery",
            tap: "",
            priority: 0,
            min_width: 0,
            token: 0,
            flags: 0,
        }
    }

    fn line() -> StatusLine<'static> {
        StatusLine {
            name: "reading",
            title: "Reading",
            height: 1000,
            token: 0,
            flags: 0,
        }
    }

    fn status_place() -> StatusPlace<'static> {
        StatusPlace {
            name: "reader",
            title: "Reader",
            left: "reader.left",
            center: "reader.center",
            right: "reader.right",
        }
    }

    #[test]
    fn widgets_accept_catalog_categories_cells_and_opaque_tokens() {
        for spec in [
            widget(),
            WidgetSpec {
                category: "My own category",
                previews: "other.preview-group",
                cells: CELL_MASK,
                token: u32::MAX,
                flags: LIVE | PRIVATE,
                ..widget()
            },
        ] {
            assert_eq!(validate("fixture", &Record::WidgetSpec(spec)), Ok(()));
        }
    }

    #[test]
    fn widgets_reject_invalid_names_references_and_reserved_bits() {
        for spec in [
            WidgetSpec {
                name: "",
                ..widget()
            },
            WidgetSpec {
                name: "Clock",
                ..widget()
            },
            WidgetSpec {
                title: "",
                ..widget()
            },
            WidgetSpec {
                category: "",
                ..widget()
            },
            WidgetSpec {
                previews: "group",
                ..widget()
            },
            WidgetSpec {
                previews: "other..group",
                ..widget()
            },
            WidgetSpec {
                cells: 1 << 16,
                ..widget()
            },
            WidgetSpec {
                flags: 1 << 2,
                ..widget()
            },
        ] {
            assert_eq!(
                validate("fixture", &Record::WidgetSpec(spec)),
                Err(ERR_INVALID)
            );
        }
        let long_owner = "x".repeat(125);
        assert_eq!(
            validate(&long_owner, &Record::WidgetSpec(widget())),
            Err(ERR_INVALID)
        );
    }

    #[test]
    fn widget_places_check_layout_and_complete_conditions() {
        for row in [
            place(),
            WidgetPlace {
                layout: 1,
                list_keys: "",
                ..place()
            },
            WidgetPlace {
                shown_if_key: "other.mode",
                shown_if_value: "widgets",
                ..place()
            },
        ] {
            assert_eq!(validate("fixture", &Record::WidgetPlace(row)), Ok(()));
        }
        for row in [
            WidgetPlace {
                name: "",
                ..place()
            },
            WidgetPlace {
                title: "",
                ..place()
            },
            WidgetPlace {
                layout: 2,
                ..place()
            },
            WidgetPlace {
                shown_if_key: "other.mode",
                ..place()
            },
            WidgetPlace {
                shown_if_value: "widgets",
                ..place()
            },
            WidgetPlace {
                shown_if_key: "mode",
                shown_if_value: "widgets",
                ..place()
            },
        ] {
            assert_eq!(
                validate("fixture", &Record::WidgetPlace(row)),
                Err(ERR_INVALID)
            );
        }
    }

    #[test]
    fn widget_place_lists_require_at_most_eight_qualified_settings() {
        for keys in [
            "setting",
            "a.key\n",
            "\na.key",
            "a.key\n\nb.key",
            "other..key",
        ] {
            assert_eq!(
                validate(
                    "fixture",
                    &Record::WidgetPlace(WidgetPlace {
                        list_keys: keys,
                        ..place()
                    })
                ),
                Err(ERR_INVALID)
            );
        }
        let keys = "a.key\na.key\na.key\na.key\na.key\na.key\na.key\na.key";
        assert_eq!(
            validate(
                "fixture",
                &Record::WidgetPlace(WidgetPlace {
                    list_keys: keys,
                    ..place()
                })
            ),
            Ok(())
        );
        let nine = alloc::format!("{keys}\na.key");
        assert_eq!(
            validate(
                "fixture",
                &Record::WidgetPlace(WidgetPlace {
                    list_keys: &nine,
                    ..place()
                })
            ),
            Err(ERR_INVALID)
        );
    }

    #[test]
    fn status_items_accept_only_private_flags_and_byte_priority() {
        for row in [
            item(),
            StatusItem {
                tap: "show-details",
                priority: 255,
                min_width: u32::MAX,
                token: u32::MAX,
                flags: PRIVATE,
                ..item()
            },
        ] {
            assert_eq!(validate("fixture", &Record::StatusItem(row)), Ok(()));
        }
        for row in [
            StatusItem { name: "", ..item() },
            StatusItem {
                title: "",
                ..item()
            },
            StatusItem {
                tap: "invalid action",
                ..item()
            },
            StatusItem {
                priority: 256,
                ..item()
            },
            StatusItem { flags: 1, ..item() },
            StatusItem { flags: 4, ..item() },
        ] {
            assert_eq!(
                validate("fixture", &Record::StatusItem(row)),
                Err(ERR_INVALID)
            );
        }
    }

    #[test]
    fn status_lines_accept_unbounded_size_and_only_private_flags() {
        for row in [
            line(),
            StatusLine {
                height: u32::MAX,
                token: u32::MAX,
                flags: PRIVATE,
                ..line()
            },
        ] {
            assert_eq!(validate("fixture", &Record::StatusLine(row)), Ok(()));
        }
        for row in [
            StatusLine { name: "", ..line() },
            StatusLine {
                title: "",
                ..line()
            },
            StatusLine { flags: 1, ..line() },
            StatusLine { flags: 4, ..line() },
        ] {
            assert_eq!(
                validate("fixture", &Record::StatusLine(row)),
                Err(ERR_INVALID)
            );
        }
    }

    #[test]
    fn status_places_name_three_qualified_settings() {
        assert_eq!(
            validate("fixture", &Record::StatusPlace(status_place())),
            Ok(())
        );
        for row in [
            StatusPlace {
                name: "",
                ..status_place()
            },
            StatusPlace {
                title: "",
                ..status_place()
            },
            StatusPlace {
                left: "",
                ..status_place()
            },
            StatusPlace {
                center: "setting",
                ..status_place()
            },
            StatusPlace {
                right: "other..setting",
                ..status_place()
            },
        ] {
            assert_eq!(
                validate("fixture", &Record::StatusPlace(row)),
                Err(ERR_INVALID)
            );
        }
    }
}
