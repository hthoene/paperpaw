


















#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugins_actions_go_with_it_and_room_can_be_made_first() {
        let mut actions = Actions::new();
        actions.try_reserve(3).unwrap();
        for id in ["gone.one", "keep.one", "gone.two"] {
            actions.insert(ActionSpec::new(Id::new_static(id), id));
        }
        actions.remove_plugin("gone");
        assert!(actions.contains("keep.one"));
        assert!(!actions.contains("gone.one"));
        assert!(!actions.contains("gone.two"));
        assert_eq!(actions.iter().count(), 1);
    }

    /// A native action is found, listed in id order among the others, runs
    /// itself through the kernel, gives way to a later action of its id and
    /// goes with its plugin (#248).
    #[test]
    fn native_actions_are_kept_without_a_handler_and_run_by_the_kernel() {
        static FIXED: [FixedAction; 1] = [FixedAction::new(Id::new_static("p.b"), "p.b")];
        let mut actions = Actions::new();
        actions.try_reserve_native(2).unwrap();
        let native = |id, token| {
            let owned = alloc::boxed::Box::new((Id::new_static(id), Cow::Borrowed(id)));
            NativeAction::new(crate::Titled::owned(owned), 7, token)
        };
        actions.insert_native(native("p.c", 3));
        actions.insert_native(native("p.a", 1));
        actions.insert_fixed(&FIXED);
        actions.insert(ActionSpec::new(Id::new_static("q.a"), "q.a"));
        let ids: Vec<&str> = actions.iter().map(|a| a.id().as_str()).collect();
        assert_eq!(ids, ["p.a", "p.b", "p.c", "q.a"]);
        let c = actions.get("p.c").unwrap();
        assert_eq!(
            (c.native(), c.title(), c.runs_itself()),
            (Some((7, 3)), "p.c", true)
        );
        assert!(actions.runnable("p.c"));
        assert_eq!(actions.get("p.b").unwrap().native(), None);
        // A later action of the same id takes its place, and the other way.
        actions.insert(ActionSpec::new(Id::new_static("p.a"), "p.a"));
        assert_eq!(actions.get("p.a").unwrap().native(), None);
        actions.insert_native(native("q.a", 9));
        assert_eq!(actions.get("q.a").unwrap().native(), Some((7, 9)));
        assert_eq!(actions.iter().count(), 4);
        actions.remove_plugin("p");
        let ids: Vec<&str> = actions.iter().map(|a| a.id().as_str()).collect();
        assert_eq!(ids, ["q.a"]);
    }

    /// A linked plugin's action: its id and title are the entry kept in the
    /// program, and it reads as a copy of the same would (#248, B).
    #[test]
    fn a_native_action_kept_in_the_program_reads_as_a_copy() {
        static KEPT: crate::TitleEntry =
            crate::TitleEntry::new(Id::new_static("p.open"), "p.open-title");
        let mut actions = Actions::new();
        actions.try_reserve_native(2).unwrap();
        actions.insert_native(NativeAction::new(crate::Titled::kept(&KEPT), 7, 3));
        let owned =
            alloc::boxed::Box::new((Id::new_static("q.open"), Cow::Borrowed("p.open-title")));
        actions.insert_native(NativeAction::new(crate::Titled::owned(owned), 7, 4));
        let kept = actions.get("p.open").unwrap();
        let copy = actions.get("q.open").unwrap();
        assert!(core::ptr::eq(kept.id(), &KEPT.id));
        assert_eq!(kept.title(), copy.title());
        assert!(matches!(kept.title_key(), Cow::Borrowed("p.open-title")));
        assert_eq!((kept.native(), copy.native()), (Some((7, 3)), Some((7, 4))));
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn a_native_action_takes_32_bytes() {
        // Its own id and title took 40 of 56 (#248, B).
        assert!(core::mem::size_of::<NativeAction>() <= 32);
    }
}
