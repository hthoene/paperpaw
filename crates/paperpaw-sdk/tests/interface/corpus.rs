//! The gate (`changes` and its helpers in `../interface.rs`) against the
//! real baselines of `interface/`, in every way a plugin built before could
//! notice and in every way of only adding to them (#228).
//!
//! Old plugins keep running after compatible additions only if two things
//! hold: whatever is not a pure addition fails CI, and pure additions
//! pass, one after the other (each bless writes the baseline anew, so the
//! second addition is compared with a baseline that already has the
//! first). The first half is a corpus of mutations of every type and
//! constant of every crate that crosses; the second applies additions in
//! sequence to each crate's baseline.
//!
//! The only changes that may pass among the mutations are the ones the
//! gate names (ADR 0026): a field appended to the firmware's table, an
//! item appended to a growing trait, and the values derived from the
//! shared table's list. Anything else that passes fails the test.

use super::*;

/// One crate's baseline.
struct Baseline {
    name: String,
    api: String,
    shape: String,
}

fn baselines() -> Vec<Baseline> {
    crates()
        .into_iter()
        .filter_map(|name| {
            Some(Baseline {
                api: read(&format!("interface/{name}.api"))?,
                shape: read(&format!("interface/{name}.shape"))?,
                name,
            })
        })
        .collect()
}

/// The path of a shape line: `struct a::B<'x>` is `a::B`.
fn path(line: &str) -> &str {
    path_of(line.split_once(' ').map_or("", |(_, rest)| rest))
}

/// Where the members of a type line begin and end (after ` { `, before
/// ` }`), if it has any in braces.
fn braces(line: &str) -> Option<(usize, usize)> {
    // The attributes of a type come first (`#{"repr": {...}}`).
    let from = line.find(" #{").map_or(0, |at| {
        line[at..].find("} { ").map_or(at, |end| at + end + 1)
    });
    let open = from + line[from..].find(" { ")? + 3;
    let close = line.strip_suffix(" }")?.len();
    (open <= close).then_some((open, close))
}

/// The members of `body`, split at the commas that are not inside
/// brackets or generics (`->` and `=>` are not brackets).
fn split(body: &str) -> Vec<&str> {
    let (mut depth, mut start, mut found) = (0i32, 0, Vec::new());
    let bytes = body.as_bytes();
    for (at, &b) in bytes.iter().enumerate() {
        match b {
            b'(' | b'[' | b'{' | b'<' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'>' if at == 0 || !matches!(bytes[at - 1], b'-' | b'=') => depth -= 1,
            b',' if depth == 0 && bytes.get(at + 1) == Some(&b' ') => {
                found.push(&body[start..at]);
                start = at + 2;
            }
            _ => {}
        }
    }
    if start < body.len() {
        found.push(&body[start..]);
    }
    found
}

/// A member to put in a type of this kind.
fn member(kind: &str, name: &str) -> String {
    match kind {
        "enum" => name.to_string(),
        "trait" => format!("function {name}(&Self) -> u8"),
        _ => format!("{name}: u8"),
    }
}

/// The ways a line of a shape file may be changed, as (name, the line
/// now; `None`: gone).
fn mutations(line: &str) -> Vec<(&'static str, Option<String>)> {
    let kind = line.split(' ').next().unwrap_or_default();
    let mut out = vec![("gone", None)];
    if kind == "const" {
        if line.contains(" = ") {
            out.push(("value", Some(format!("{line}_changed"))));
        }
        return out;
    }
    let Some((open, close)) = braces(line) else {
        out.push(("renamed", Some(format!("{line}_changed"))));
        return out;
    };
    let members = split(&line[open..close]);
    let with = |body: String| format!("{}{body}{}", &line[..open], &line[close..]);
    out.push((
        "first member added",
        Some(with(format!(
            "{}, {}",
            member(kind, "zz_first"),
            &line[open..close]
        ))),
    ));
    out.push((
        "member appended",
        Some(with(if members.is_empty() {
            member(kind, "zz_last")
        } else {
            format!("{}, {}", &line[open..close], member(kind, "zz_last"))
        })),
    ));
    if !members.is_empty() {
        out.push((
            "last member gone",
            Some(with(members[..members.len() - 1].join(", "))),
        ));
        let mut retyped = members.clone();
        let last = format!("{}_changed", members[members.len() - 1]);
        retyped.pop();
        retyped.push(&last);
        out.push(("last member changed", Some(with(retyped.join(", ")))));
    }
    if members.len() >= 2 && members[0] != members[1] {
        let mut swapped = members.clone();
        swapped.swap(0, 1);
        out.push(("first two swapped", Some(with(swapped.join(", ")))));
    }
    out
}

/// Whether the gate lets this change through by name (ADR 0026), not as a
/// gap: the table of the firmware grows at its end, a growing trait
/// gains an item at its end, the table's derived constants follow
/// `shared.list`.
fn named_exception(crate_: &str, line: &str, what: &str) -> bool {
    let kind_and_path = format!(
        "{} {}",
        line.split(' ').next().unwrap_or_default(),
        path(line)
    );
    match what {
        "member appended" => {
            kind_and_path == API_SHAPE
                || (GROWING.contains(&kind_and_path.as_str()) && kind_and_path != SCREEN_TRAIT)
        }
        "value" | "gone" | "renamed" => {
            what == "value" && table_index_derived(crate_, &kind_and_path)
        }
        _ => false,
    }
}

#[test]
fn nothing_but_an_addition_passes_the_gate() {
    let all = baselines();
    if all.is_empty() {
        return;
    }
    let (mut tried, mut excepted) = (0, Vec::new());
    let mut gaps = Vec::new();
    for b in &all {
        let lines: Vec<&str> = b.shape.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            for (what, mutated) in mutations(line) {
                let mut now: Vec<&str> = lines.clone();
                match &mutated {
                    Some(text) => now[i] = text,
                    None => {
                        now.remove(i);
                    }
                }
                let now = format!("{}\n", now.join("\n"));
                let found = changes(&b.name, ("", &b.shape), ("", &now));
                tried += 1;
                if found.breaking.is_empty() {
                    if named_exception(&b.name, line, what) {
                        excepted.push(format!("{} {} ({what})", b.name, path(line)));
                    } else {
                        gaps.push(format!("{}: {what}: {line}", b.name));
                    }
                }
            }
        }
    }
    assert!(
        gaps.is_empty(),
        "changes a plugin built before could notice, that the gate lets through ({} of {tried}):\n{}",
        gaps.len(),
        gaps.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
    // The corpus is not empty, and the exceptions are the ones named.
    assert!(tried > 5_000, "only {tried} mutations");
    excepted.sort();
    excepted.dedup();
    for needed in [
        "paperpaw-native paperpaw_native::interface::Api (member appended)",
        "paperpaw-text paperpaw_text::cache::Cache (member appended)",
        "paperpaw-text paperpaw_text::stored::Pack (member appended)",
    ] {
        assert!(
            excepted.iter().any(|e| e == needed),
            "{needed}: {excepted:?}"
        );
    }
    // `table_index_derived` constants are the only others.
    for e in &excepted {
        assert!(
            e.contains("(member appended)")
                || e.starts_with("paperpaw-table paperpaw_table::index"),
            "{e}"
        );
    }
}

#[test]
fn an_api_line_changed_or_gone_fails_the_gate() {
    let all = baselines();
    let mut tried = 0;
    let mut gaps = Vec::new();
    for b in &all {
        let lines: Vec<&str> = b.api.lines().collect();
        // Every 17th line, to keep the test quick.
        for (i, line) in lines.iter().enumerate().filter(|(i, _)| i % 17 == 0) {
            for now_line in [None, Some(format!("{line} /* changed */"))] {
                let mut now = lines.clone();
                match &now_line {
                    Some(text) => now[i] = text,
                    None => {
                        now.remove(i);
                    }
                }
                let now = format!("{}\n", now.join("\n"));
                tried += 1;
                if changes(&b.name, (&b.api, ""), (&now, ""))
                    .breaking
                    .is_empty()
                {
                    gaps.push(format!("{}: {line}", b.name));
                }
            }
        }
    }
    assert!(gaps.is_empty(), "{gaps:#?}");
    assert!(tried > 700, "only {tried}");
}

/// The lines of `text` with `more` after them.
fn plus(text: &str, more: &[String]) -> String {
    let mut out = text.to_string();
    for line in more {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// A shape line's members with `added` at their end.
fn grown(line: &str, added: &str) -> String {
    let (_, close) = braces(line).expect("a type with members");
    format!("{}, {added}{}", &line[..close], &line[close..])
}

/// Replaces the line of `text` that starts with `start` by `with`.
fn replace_line(text: &str, start: &str, with: &str) -> String {
    let mut found = false;
    let out: String = text
        .lines()
        .map(|l| {
            if l.starts_with(start) {
                found = true;
                format!("{with}\n")
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    assert!(found, "no line starts with {start}");
    out
}

/// A step: what it is, the lines it adds to the shape, those it adds to the API.
type Step = (String, Vec<String>, Vec<String>);

/// The blessed additions made to one baseline, one after the other.
fn addition_steps(b: &Baseline, holder: Option<&String>) -> Vec<Step> {
    let mut steps: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
    for n in 1..=3 {
        let z = format!("zz{n}");
        steps.push((
            format!("a new type, step {n}"),
            vec![format!("struct {z}::Fresh {{ a: u8, b: u16 }}")],
            vec![
                format!("pub struct {z}::Fresh"),
                format!("pub {z}::Fresh::a: u8"),
                format!("pub {z}::Fresh::b: u16"),
                format!("pub fn {z}::Fresh::new() -> {z}::Fresh"),
                format!("impl core::fmt::Debug for {z}::Fresh"),
            ],
        ));
        steps.push((
            format!("free items, step {n}"),
            vec![],
            vec![
                format!("pub fn {z}::free() -> u8"),
                format!("pub const {z}::LIMIT: u8"),
                format!("pub mod {z}::inner"),
                format!("pub use {z}::Fresh"),
            ],
        ));
        if let Some(holder) = holder {
            steps.push((
                format!("a method of a type that was there, step {n}"),
                vec![],
                vec![format!("pub fn {holder}::added_{n}(&self) -> u8")],
            ));
        }
    }
    // The firmware's table: three fields one after the other.
    if b.name == "paperpaw-native" {
        for (n, ty) in [
            (1, "u32"),
            (2, "*const u8"),
            (3, "unsafe extern \"C\" fn(usize) -> u8"),
        ] {
            steps.push((
                format!("a field of the table, step {n}"),
                vec![format!("@{API_SHAPE}|probe_{n}: {ty}")],
                vec![format!("pub paperpaw_native::Api::probe_{n}: {ty}")],
            ));
        }
    }
    // Items at the end of the growing traits.
    let growing: &[(&str, bool)] = match b.name.as_str() {
        "paperpaw-text" => &[
            ("trait paperpaw_text::stored::Pack", false),
            ("trait paperpaw_text::cache::Cache", false),
        ],
        "paperpaw-sdk" => &[(SCREEN_TRAIT, true)],
        _ => &[],
    };
    for &(trait_, revision) in growing {
        for n in 1..=3 {
            steps.push((
                format!("an item at the end of {trait_}, step {n}"),
                vec![format!(
                    "@{trait_}|function appended_{n}(&Self) -> u8{}",
                    if revision { "@revision" } else { "" }
                )],
                vec![],
            ));
        }
    }
    steps
}

/// One blessed addition after another: new types, functions, constants,
/// methods of types that were there, the table's fields, items of growing
/// traits (and the `Screen` revision), each compared with the baseline
/// as the one before it wrote it.
#[test]
fn additions_pass_one_after_the_other() {
    for b in baselines() {
        let (mut api, mut shape) = (b.api.clone(), b.shape.clone());
        let holder = shape
            .lines()
            .find(|l| l.starts_with("struct ") || l.starts_with("enum "))
            .map(|l| path(l).to_string());
        let steps = addition_steps(&b, holder.as_ref());
        for (what, shape_more, api_more) in steps {
            let mut now_shape = shape.clone();
            let mut plain = Vec::new();
            for line in &shape_more {
                if let Some(rest) = line.strip_prefix('@') {
                    let (start, member) = rest.split_once('|').unwrap();
                    let (member, revision) = member
                        .strip_suffix("@revision")
                        .map_or((member, false), |m| (m, true));
                    // The type's own line, not another that begins with its name.
                    let start = format!("{start} ");
                    let old = now_shape
                        .lines()
                        .find(|l| l.starts_with(&start))
                        .unwrap()
                        .to_string();
                    now_shape = replace_line(&now_shape, &start, &grown(&old, member));
                    if revision {
                        let number = now_shape
                            .lines()
                            .find_map(|l| {
                                l.strip_prefix(SCREEN_REVISION_PATH)?
                                    .strip_prefix(": u16 = ")?
                                    .strip_suffix("u16")?
                                    .parse::<u16>()
                                    .ok()
                            })
                            .unwrap();
                        now_shape = replace_line(
                            &now_shape,
                            SCREEN_REVISION_PATH,
                            &format!("{SCREEN_REVISION_PATH}: u16 = {}u16", number + 1),
                        );
                    }
                } else {
                    plain.push(line.clone());
                }
            }
            now_shape = plus(&now_shape, &plain);
            let now_api = plus(&api, &api_more);
            let found = changes(&b.name, (&api, &shape), (&now_api, &now_shape));
            assert!(
                found.breaking.is_empty(),
                "{}: {what} is only an addition: {:?}",
                b.name,
                found.breaking
            );
            assert!(!found.new.is_empty(), "{}: {what} is seen as new", b.name);
            // Blessed: the next step starts from this.
            (api, shape) = (now_api, now_shape);
        }
    }
}

/// Blessing an addition rewrites the baseline, so the next addition is
/// compared with a table that already has the first one's field: a
/// second, a third field appended is as free as the first (#228).
#[test]
fn the_firmware_s_table_grows_again_after_it_grew() {
    let path = "struct paperpaw_native::interface::Api #{}";
    let table = |fields: &str| format!("{path} {{ size: u32, build_len: u32{fields} }}");
    let changed = |before: &str, after: &str| changes("paperpaw-native", ("", before), ("", after));
    let once = table(", more: u8");
    let twice = table(", more: u8, again: u16");
    let thrice = table(", more: u8, again: u16, last: [u8; 3]");
    for (before, after) in [
        (table(""), once.clone()),
        (once.clone(), twice.clone()),
        (twice.clone(), thrice.clone()),
        (table(""), thrice.clone()),
    ] {
        let found = changed(&before, &after);
        assert!(
            found.breaking.is_empty(),
            "{before} to {after}: {:?}",
            found.breaking
        );
        assert_eq!(found.new.len(), 1, "{before} to {after}");
    }
    // What was appended stays where it is, with its type; a field that
    // was there cannot be dropped, moved or retyped, and a field before
    // the end of interface 5 is never new.
    for (before, after) in [
        (twice.clone(), once.clone()),
        (twice.clone(), table(", again: u16, more: u8")),
        (twice.clone(), table(", more: u16, again: u16")),
        (twice.clone(), table(", more: u8, mid: u8, again: u16")),
        (
            once.clone(),
            format!("{path} {{ size: u32, early: u8, build_len: u32, more: u8 }}"),
        ),
    ] {
        assert!(
            !changed(&before, &after).breaking.is_empty(),
            "{before} to {after}"
        );
    }
    // The same through the api lines: a field of the table that is new
    // beside the ones already appended.
    let kept = types(&twice);
    assert!(api_field_appended(
        "pub paperpaw_native::Api::last: [u8; 3]",
        &kept
    ));
    assert!(!api_field_appended(
        "pub paperpaw_native::Api::again: u16",
        &kept
    ));
}
