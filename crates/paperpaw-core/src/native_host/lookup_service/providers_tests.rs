//! The production lookup host over the declaration store: the providers left
//! for an ask (the asker's own, the network ones without a network and the
//! ones of another language out), in load order, one per page.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
extern crate std;
use super::*;
use crate::native_host::declaration_store::{
    Allocations, DeclarationStore, ImageOwner, PendingDeclarations,
};
use crate::native_host::registrations::{
    LookupSpec, REGISTRATION_MAX_BYTES, Record, records_encode,
};
use alloc::{rc::Rc, string::String, vec, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Id, Settings};

struct Plugin {
    id: &'static str,
    host: u64,
    live: Cell<bool>,
}
impl ImageOwner for Plugin {
    fn plugin_id(&self) -> &str {
        self.id
    }
    fn generation(&self) -> u32 {
        u32::from(self.live.get())
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(self.host)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(self.live.get().then_some(&[][..]));
    }
    fn invalid_declaration(&self, _: i32) {}
}

fn install(store: &mut DeclarationStore, plugin: &Rc<Plugin>, rows: &[(&str, &str, u32, u32)]) {
    let records: Vec<Record<'_>> = rows
        .iter()
        .map(|(name, languages, flags, token)| {
            Record::LookupSpec(LookupSpec {
                name,
                title: "Title",
                languages,
                flags: *flags,
                token: *token,
            })
        })
        .collect();
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(&records, &mut bytes).unwrap();
    bytes.truncate(len);
    let mut allocations = Allocations::default();
    let mut pending = PendingDeclarations::new(Id::try_parse(plugin.id).unwrap(), plugin.clone());
    pending.stage_batch(&bytes, &mut allocations).unwrap();
    store.prepare(pending, &mut allocations).unwrap().commit();
}

/// One call of a provider: owner, token, text, language, cursor.
type Call = (u64, u32, String, String, u32);

/// A frame over a declaration store whose providers answer
/// `title of <owner>` (or fail with `failing`'s status).
struct Rig<'a> {
    env: &'a Env<'a>,
    store: DeclarationStore,
    /// (owner, token, text, language, cursor) of every call.
    calls: RefCell<Vec<Call>>,
    /// The owner whose provider fails, and how; the flags of every answer.
    failing: Cell<Option<(u64, i32)>>,
    flags: Cell<u32>,
    /// The owner whose provider answers bytes that are no `lookup_entry`.
    garbage: Cell<u64>,
}
impl Frame for Rig<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn kernel_env(&self) -> Option<&Env<'_>> {
        Some(self.env)
    }
    fn with_declarations(&self, visit: &mut dyn FnMut(&dyn core::any::Any)) -> Result<(), i32> {
        visit(&self.store);
        Ok(())
    }
    fn call_native(
        &self,
        owner: u64,
        token: u32,
        input: &[u8],
        reply: &mut [u8],
        accept: &mut dyn FnMut(&[u8]) -> Result<(), i32>,
    ) -> Result<usize, i32> {
        let Record::StageCall(call) = wire::records_decode_single(input).unwrap() else {
            panic!("not a stage.call")
        };
        assert_eq!((call.stage, call.schema), ("core.lookup", 1));
        let asked = stage::LookupAsk::decode(call.payload).unwrap();
        self.calls.borrow_mut().push((
            owner,
            token,
            asked.text.into(),
            asked.language.into(),
            asked.cursor,
        ));
        if let Some((who, status)) = self.failing.get()
            && who == owner
        {
            return Err(status);
        }
        if self.garbage.get() == owner {
            reply[..3].copy_from_slice(&[0xff; 3]);
            accept(&reply[..3])?;
            return Ok(3);
        }
        let title = std::format!("title of {owner}");
        let length = stage::LookupEntry::new(&title, "text", self.flags.get(), "wikipedia.article")
            .encode(reply)
            .unwrap();
        accept(&reply[..length])?;
        Ok(length)
    }
}

const ME: u64 = 9;

fn kernel(network: bool) -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .network(network)
        .build()
        .unwrap()
}

/// The page of `cursor` for the asker `ME` in `language`: the provider, the
/// entry's title and flags, or the status.
fn page(rig: &Rig<'_>, language: &str, cursor: u32) -> Result<(String, String, u32), i32> {
    let ask = wire::ServiceLookupAsk {
        text: "word",
        language,
        context: "around",
        cursor,
    };
    let mut out = vec![0; 4096];
    let length = Providers.ask(rig, ME, &ask, &mut out)?;
    let Record::ServiceLookupEntry(entry) = wire::record_decode_one(&out[..length]).unwrap() else {
        panic!("not a lookup_entry")
    };
    Ok((entry.provider.into(), entry.title.into(), entry.flags))
}

fn rig<'a>(env: &'a Env<'a>) -> Rig<'a> {
    Rig {
        env,
        store: DeclarationStore::default(),
        calls: RefCell::new(Vec::new()),
        failing: Cell::new(None),
        flags: Cell::new(0),
        garbage: Cell::new(0),
    }
}

fn plugin(id: &'static str, host: u64) -> Rc<Plugin> {
    Rc::new(Plugin {
        id,
        host,
        live: Cell::new(true),
    })
}

#[test]
fn providers_answer_one_per_page_in_load_order_and_the_last_has_no_more() {
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let mut rig = rig(&env);
    install(&mut rig.store, &plugin("dict", 1), &[("main", "", 0, 11)]);
    install(
        &mut rig.store,
        &plugin("wiki", 2),
        &[("wiki", "", 0, 22), ("extra", "", 0, 23)],
    );
    assert_eq!(
        page(&rig, "en", 0),
        Ok(("main".into(), "title of 1".into(), 1))
    );
    assert_eq!(
        page(&rig, "en", 1),
        Ok(("wiki".into(), "title of 2".into(), 1))
    );
    assert_eq!(
        page(&rig, "en", 2),
        Ok(("extra".into(), "title of 2".into(), 0))
    );
    assert_eq!(page(&rig, "en", 3), Err(ERR_NOT_FOUND));
    // The provider got the ask with its own token and cursor 0 (its paging stays in its page).
    let calls = rig.calls.borrow();
    assert_eq!(calls[1], (2, 22, "word".into(), "en".into(), 0));
    assert_eq!(calls.len(), 3);
}

#[test]
fn the_screen_an_entry_opens_reaches_the_asker() {
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let mut rig = rig(&env);
    install(&mut rig.store, &plugin("wiki", 2), &[("wiki", "", 0, 2)]);
    let ask = wire::ServiceLookupAsk {
        text: "word",
        language: "",
        context: "",
        cursor: 0,
    };
    let mut out = vec![0; 4096];
    let length = Providers.ask(&rig, ME, &ask, &mut out).unwrap();
    let Record::ServiceLookupEntry(entry) = wire::record_decode_one(&out[..length]).unwrap() else {
        panic!("not a lookup_entry")
    };
    // As the provider named it (`document.md` 7: Beta 3's *Open Wikipedia ›*).
    assert_eq!(entry.opens, "wikipedia.article");
}

#[test]
fn a_provider_always_gets_cursor_0_whatever_its_index() {
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let mut rig = rig(&env);
    install(&mut rig.store, &plugin("dict", 1), &[("main", "", 0, 1)]);
    install(&mut rig.store, &plugin("wiki", 2), &[("wiki", "", 0, 2)]);
    page(&rig, "", 0).unwrap();
    page(&rig, "", 1).unwrap();
    let calls = rig.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|call| call.4 == 0), "{calls:?}");
}

#[test]
fn the_providers_own_more_is_not_passed_on() {
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let mut rig = rig(&env);
    install(&mut rig.store, &plugin("dict", 1), &[("main", "", 0, 1)]);
    rig.flags.set(1);
    // The only provider: its own "more pages" is the host's to set (none).
    assert_eq!(
        page(&rig, "", 0),
        Ok(("main".into(), "title of 1".into(), 0))
    );
}

#[test]
fn a_network_provider_is_left_out_while_no_network_is_connected() {
    let off = kernel(false);
    let env = off.shared.env(Instant(0));
    let mut rig_off = rig(&env);
    let on = kernel(true);
    let env_on = on.shared.env(Instant(0));
    let mut rig_on = rig(&env_on);
    for rig in [&mut rig_off, &mut rig_on] {
        install(&mut rig.store, &plugin("dict", 1), &[("main", "", 0, 1)]);
        install(&mut rig.store, &plugin("wiki", 2), &[("wiki", "", 1, 2)]);
    }
    assert_eq!(
        page(&rig_off, "", 0),
        Ok(("main".into(), "title of 1".into(), 0))
    );
    assert_eq!(page(&rig_off, "", 1), Err(ERR_NOT_FOUND));
    assert_eq!(
        page(&rig_on, "", 0),
        Ok(("main".into(), "title of 1".into(), 1))
    );
    assert_eq!(
        page(&rig_on, "", 1),
        Ok(("wiki".into(), "title of 2".into(), 0))
    );
}

#[test]
fn a_provider_takes_the_language_or_its_base_and_an_empty_ask_takes_all() {
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let mut rig = rig(&env);
    install(
        &mut rig.store,
        &plugin("dict", 1),
        &[
            ("de", "de", 0, 1),
            ("en", "en\nfr", 0, 2),
            ("any", "", 0, 3),
        ],
    );
    let names = |language: &str| {
        (0..4)
            .map_while(|cursor| page(&rig, language, cursor).ok().map(|page| page.0))
            .collect::<Vec<_>>()
    };
    // `de-AT` is German too, whatever the case.
    assert_eq!(names("de-AT"), ["de", "any"]);
    assert_eq!(names("DE"), ["de", "any"]);
    assert_eq!(names("fr_CA"), ["en", "any"]);
    assert_eq!(names("ja"), ["any"]);
    assert_eq!(names(""), ["de", "en", "any"]);
}

#[test]
fn the_askers_own_providers_and_stopped_plugins_are_left_out() {
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let mut rig = rig(&env);
    let gone = plugin("gone", 3);
    install(&mut rig.store, &plugin("me", ME), &[("mine", "", 0, 1)]);
    install(&mut rig.store, &gone, &[("gone", "", 0, 2)]);
    install(&mut rig.store, &plugin("dict", 1), &[("main", "", 0, 3)]);
    gone.live.set(false);
    assert_eq!(
        page(&rig, "", 0),
        Ok(("main".into(), "title of 1".into(), 0))
    );
    assert_eq!(page(&rig, "", 1), Err(ERR_NOT_FOUND));
    assert!(rig.calls.borrow().iter().all(|call| call.0 == 1));
}

#[test]
fn a_provider_that_fails_or_answers_badly_is_an_empty_page() {
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let mut rig = rig(&env);
    install(&mut rig.store, &plugin("dict", 1), &[("main", "", 0, 1)]);
    install(&mut rig.store, &plugin("wiki", 2), &[("wiki", "", 0, 2)]);
    // Its own "not found" never ends the asker's loop: an empty entry.
    rig.failing.set(Some((1, ERR_NOT_FOUND)));
    assert_eq!(page(&rig, "", 0), Ok(("main".into(), String::new(), 1)));
    assert_eq!(
        page(&rig, "", 1),
        Ok(("wiki".into(), "title of 2".into(), 0))
    );
    // Its other failures are its page only too: the ones after it still
    // answer.
    for status in [ERR_BUSY, ERR_NO_MEMORY, ERR_INVALID] {
        rig.failing.set(Some((1, status)));
        assert_eq!(page(&rig, "", 0), Ok(("main".into(), String::new(), 1)));
    }
    rig.failing.set(Some((2, ERR_BUSY)));
    assert_eq!(page(&rig, "", 1), Ok(("wiki".into(), String::new(), 0)));
    // So is a reply the stage check or the decoder refuses.
    rig.failing.set(None);
    rig.garbage.set(1);
    assert_eq!(page(&rig, "", 0), Ok(("main".into(), String::new(), 1)));
    assert_eq!(
        page(&rig, "", 1),
        Ok(("wiki".into(), "title of 2".into(), 0))
    );
}

#[test]
fn a_frame_without_declarations_cannot_say_who_provides() {
    struct Bare<'a>(&'a Env<'a>);
    impl Frame for Bare<'_> {
        fn settings(&self) -> &Settings {
            self.0.settings()
        }
        fn translator(&self) -> &Translator {
            self.0.i18n()
        }
    }
    let kernel = kernel(false);
    let env = kernel.shared.env(Instant(0));
    let ask = wire::ServiceLookupAsk {
        text: "word",
        language: "",
        context: "",
        cursor: 0,
    };
    let mut out = vec![0; 4096];
    assert_eq!(
        Providers.ask(&Bare(&env), ME, &ask, &mut out),
        Err(ERR_UNSUPPORTED)
    );
}
