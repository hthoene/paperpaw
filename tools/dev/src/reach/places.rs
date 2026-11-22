//! The second search for the ways the buttons lead (see [`super`]): a
//! walk through the places of the app, each searched once.
//!
//! A *place* is what a press can tell apart: the screen shown with its
//! parts, the screen it lies over, and the part with the focus
//! ([`Seen::place`]). The walk keeps what it learnt: where each press led
//! from each place. It always presses in the nearest place that has a
//! button left (along the presses learnt; else up with back and back
//! with the buttons that move the focus backward, where those are not
//! pressed yet; else from the home screen; else from there on a fresh
//! device), so nothing is pressed twice but to get somewhere. So it
//! goes deep, where the first search goes wide: into a book, its menu,
//! the contents.
//!
//! # What is pressed, in which order
//!
//! Every button is pressed in every place, short and long, but in turns
//! through the whole app, what finds screens first:
//!
//! 1. confirm, and the button that moves the focus forward, with the page
//!    key that goes on where it goes round a page (the arrow keys stay on
//!    the page, #465): every part of every screen is acted on. On a device
//!    without a confirm button, every long press but back's (they act
//!    there).
//! 2. confirm held, the other buttons that move the focus forward (a
//!    page of a list at a time), and what a device has besides (a menu
//!    button).
//! 3. The buttons that move the focus backward.
//! 4. The rest: back, and the other long presses.
//!
//! A screen with the same parts over another screen (the keyboard,
//! opened from many; a menu) is searched so once, where it is first
//! seen: over the others only the first turn is pressed, as what a part
//! does there differs with the screen below (*OK* on the keyboard).
//!
//! # Where it ends
//!
//! When every screen looked for is found; when nothing is left to press
//! (then it is *complete*: a screen not found has no way to it); or at
//! its bound, [`Limits::steps`] presses. PaperPaw's app has thousands of
//! places (the keyboard alone a thousand, with every letter's variants):
//! measured on it (#351), the first turn takes some 10,000 presses, a
//! minute, and finds what the walk finds at all but for three or four
//! screens that 150,000 presses (a quarter of an hour) add; to the end
//! it does not come. So there it ends at its bound, and a screen not
//! found is not decided: [`Found::open`] says why.

use std::collections::{HashMap, HashSet, VecDeque};

use paperpaw_platform::buttons;

use super::{Driver, Found, Limits, Seen, focus_like, forward, movers, page_key};

/// A place and what was learnt of it.
struct Place {
    /// As it was seen first.
    seen: Seen,
    /// The way it was first come by, from the home screen.
    way: Vec<String>,
    /// Where each press led: the button (its number in the buttons
    /// pressed), the place.
    moves: Vec<(usize, usize)>,
    /// The first button not pressed here yet.
    first: usize,
    /// No way led to it again.
    lost: bool,
    /// It is the first place seen with this screen, parts and focus,
    /// over whatever screen.
    own: bool,
}

/// The turns the buttons are pressed in.
const TURNS: u8 = 4;

struct Walk<'d, 'c> {
    driver: &'d mut Driver<'c>,
    /// The buttons pressed, in the order of their turns.
    keys: Vec<String>,
    /// The turn of each.
    turn: Vec<u8>,
    /// The buttons that move the focus backward, and back: pressed to
    /// get back and up where no way is learnt yet.
    backward: Vec<usize>,
    back: Option<usize>,
    /// The first that moves the focus forward.
    forward: Option<String>,
    places: Vec<Place>,
    index: HashMap<String, usize>,
    likes: HashSet<String>,
    views: HashSet<String>,
    /// The place the device is at, and what it shows.
    at: Option<usize>,
    now: Option<Seen>,
    /// The driver's steps when the search began.
    began: usize,
    found: Found,
}

/// The turn `key` is pressed in on `variant`'s device (see the module's
/// text), from 0.
fn turn(variant: &super::Variant, key: &str) -> u8 {
    let has = |button: &str| variant.profile.buttons.contains(&button);
    let forward = forward(variant);
    let page = forward.first().and_then(|f| page_key(variant, f));
    let long = key.starts_with("long:");
    let button = key.strip_prefix("long:").unwrap_or(key);
    let moves = movers(variant).iter().any(|m| m == button);
    if Some(key) == super::back_key(variant).as_deref() || button == buttons::BACK {
        return 3;
    }
    match (long, moves) {
        (false, false) if button == buttons::CONFIRM || !has(buttons::CONFIRM) => 0,
        (false, true) if forward.first().is_some_and(|f| f == button) => 0,
        (false, true) if page.as_deref() == Some(button) => 0,
        (true, _) if !has(buttons::CONFIRM) => 0,
        (true, false) | (false, false) => 1,
        (false, true) if forward.iter().any(|f| f == button) => 1,
        (false, true) => 2,
        (true, true) => 3,
    }
}

/// Searches the ways from the home screen until `wanted` screens are all
/// found, nothing is left to press, or the bound is reached (see the
/// module's text). The settings are kept as at the start (see
/// [`Driver::keep_settings`]).
pub fn search(driver: &mut Driver<'_>, wanted: &HashSet<String>, limits: &Limits) -> Found {
    let variant = driver.variant.clone();
    // What was done on the device before is left behind.
    driver.fresh();
    driver.keep_settings();
    // Every button short and long, but power.
    let pressed = || {
        variant
            .profile
            .buttons
            .iter()
            .filter(|b| **b != buttons::POWER)
    };
    let mut keys: Vec<String> = pressed()
        .map(ToString::to_string)
        .chain(pressed().map(|button| format!("long:{button}")))
        .collect();
    keys.sort_by_key(|key| turn(&variant, key));
    let number = |key: &str| keys.iter().position(|k| k == key);
    let forward = forward(&variant);
    let backward = movers(&variant)
        .iter()
        .filter(|m| !forward.contains(m))
        .filter_map(|m| number(m))
        .collect();
    let back = super::back_key(&variant).and_then(|back| number(&back));
    let began = driver.steps;
    let mut walk = Walk {
        driver,
        turn: keys.iter().map(|key| turn(&variant, key)).collect(),
        keys,
        backward,
        back,
        forward: forward.into_iter().next(),
        places: Vec::new(),
        index: HashMap::new(),
        likes: HashSet::new(),
        views: HashSet::new(),
        at: None,
        now: None,
        began,
        found: Found::default(),
    };
    walk.run(wanted, limits);
    walk.finish(wanted)
}

impl Walk<'_, '_> {
    fn run(&mut self, wanted: &HashSet<String>, limits: &Limits) {
        let Some(start) = self.driver.go(&[], None) else {
            self.found.open = Some(String::from("the home screen did not show"));
            return;
        };
        self.at = Some(self.note(&start, Vec::new()));
        // From the home screen, sleep: what it shows then was reached
        // (the device is woken again at once).
        if self
            .driver
            .variant
            .profile
            .buttons
            .contains(&buttons::POWER)
        {
            self.driver.step(buttons::POWER);
            for screen in &self.driver.asleep {
                self.found
                    .ways
                    .entry(screen.clone())
                    .or_insert_with(|| (vec![buttons::POWER.to_string()], String::new()));
            }
            let seen = self.driver.observe();
            self.at = Some(self.note(&seen, vec![buttons::POWER.to_string()]));
        }
        let left = |walk: &Self| {
            (0..walk.places.len())
                .filter(|&place| walk.due(place, TURNS))
                .count()
        };
        loop {
            if wanted.iter().all(|w| self.found.ways.contains_key(w)) {
                return;
            }
            if self.driver.steps - self.began >= limits.steps {
                self.found.open = Some(format!(
                    "the search ended at its bound of {} presses, with buttons left to press in \
                     {} of the {} places it saw",
                    limits.steps,
                    left(self),
                    self.places.len()
                ));
                return;
            }
            let Some(place) = self.next() else {
                break;
            };
            if self.arrive(place) {
                self.press(place, self.places[place].first);
            } else {
                self.places[place].lost = true;
            }
        }
        let lost = self.places.iter().filter(|p| p.lost).count();
        if lost > 0 {
            self.found.open = Some(format!(
                "{lost} of the {} places the search saw were not come to again to press every \
                 button there (what was pressed before had changed the way)",
                self.places.len()
            ));
        }
        self.found.complete = self.found.open.is_none();
    }

    /// The place `seen` is, new or known; `way` leads to it.
    fn note(&mut self, seen: &Seen, way: Vec<String>) -> usize {
        self.views.insert(seen.view());
        self.found
            .ways
            .entry(seen.top().to_string())
            .or_insert_with(|| (way.clone(), seen.state()));
        self.now = Some(seen.clone());
        let key = seen.place();
        if let Some(&known) = self.index.get(&key) {
            return known;
        }
        // The same over whatever screen: without the screen below.
        let like = key.split_once('>').map_or(key.as_str(), |(_, like)| like);
        let own = self.likes.insert(like.to_string());
        self.index.insert(key, self.places.len());
        self.places.push(Place {
            seen: seen.clone(),
            way,
            moves: Vec::new(),
            first: 0,
            lost: false,
            own,
        });
        self.places.len() - 1
    }

    /// Whether a button of a turn up to `turn` is left to press in
    /// `place`: of the first turn only, where the place is like one seen
    /// before over another screen.
    fn due(&self, place: usize, turn: u8) -> bool {
        let known = &self.places[place];
        let turn = if known.own { turn } else { 0 };
        !known.lost && self.turn.get(known.first).is_some_and(|&t| t <= turn)
    }

    /// Presses button `key` in place `from`, where the device is, and
    /// learns where it led.
    fn press(&mut self, from: usize, key: usize) {
        let name = self.keys[key].clone();
        let was = self
            .now
            .clone()
            .unwrap_or_else(|| self.places[from].seen.clone());
        // Nothing described, but something like a focus moves: the
        // search cannot follow it.
        let watched = self.forward.as_ref() == Some(&name)
            && was.targets().next().is_none()
            && !self.found.undescribed.contains_key(was.top());
        let before = watched.then(|| self.driver.session.image());
        self.driver.step(&name);
        let seen = self.driver.observe();
        let mut way = self.places[from].way.clone();
        if let Some(before) = before {
            let after = self.driver.session.image();
            if seen.stack == was.stack
                && seen.targets().next().is_none()
                && focus_like(&before, &after)
            {
                self.found
                    .undescribed
                    .insert(seen.top().to_string(), (way.clone(), name.clone(), after));
            }
        }
        way.push(name);
        let to = self.note(&seen, way);
        let place = &mut self.places[from];
        match place.moves.iter_mut().find(|(pressed, _)| *pressed == key) {
            Some(known) => known.1 = to,
            None => place.moves.push((key, to)),
        }
        while place
            .moves
            .iter()
            .any(|(pressed, _)| *pressed == place.first)
        {
            place.first += 1;
        }
        self.at = Some(to);
    }

    /// The place to press in next: of the earliest turn that has a button
    /// left anywhere, the nearest along the presses learnt, else the
    /// first seen.
    fn next(&self) -> Option<usize> {
        let turn = (0..TURNS).find(|&turn| (0..self.places.len()).any(|p| self.due(p, turn)))?;
        if let Some(at) = self.at {
            let mut seen = vec![false; self.places.len()];
            let mut queue = VecDeque::from([at]);
            seen[at] = true;
            while let Some(place) = queue.pop_front() {
                if self.due(place, turn) {
                    return Some(place);
                }
                for &(_, to) in &self.places[place].moves {
                    if !seen[to] {
                        seen[to] = true;
                        queue.push_back(to);
                    }
                }
            }
        }
        (0..self.places.len()).find(|&place| self.due(place, turn))
    }

    /// The presses learnt that lead from `from` to `to`, the fewest.
    fn path(&self, from: usize, to: usize) -> Option<Vec<usize>> {
        let mut came: Vec<Option<(usize, usize)>> = vec![None; self.places.len()];
        let mut queue = VecDeque::from([from]);
        while let Some(place) = queue.pop_front() {
            if place == to {
                let mut path = Vec::new();
                let mut at = to;
                while at != from {
                    let (before, key) = came[at]?;
                    path.push(key);
                    at = before;
                }
                path.reverse();
                return Some(path);
            }
            for &(key, next) in &self.places[place].moves {
                if next != from && came[next].is_none() {
                    came[next] = Some((place, key));
                    queue.push_back(next);
                }
            }
        }
        None
    }

    /// Brings the device to place `to` along the presses learnt: from
    /// where it is (unless home is nearer); where no way is learnt yet,
    /// first up with back, or on the same screen back with the buttons
    /// that move the focus backward (those not pressed there yet); else
    /// from the home screen; else from there on a fresh device.
    fn arrive(&mut self, to: usize) -> bool {
        let from_home = self.path(0, to).map(|path| path.len() + 1);
        for _ in 0..6 {
            let Some(at) = self.at else {
                break;
            };
            if at == to {
                return true;
            }
            match self.path(at, to) {
                Some(path) if from_home.is_none_or(|home| path.len() <= home) => {
                    if self.follow(to) {
                        return true;
                    }
                    break;
                }
                Some(_) => break,
                None => {}
            }
            let (here, there) = (&self.places[at].seen, &self.places[to].seen);
            let same = here.top() == there.top() && here.stack.len() == there.stack.len();
            let pressed = |key: &usize| self.places[at].moves.iter().any(|(k, _)| k == key);
            let help = if same {
                self.backward.iter().copied().find(|key| !pressed(key))
            } else if here.stack.len() > 1 {
                self.back.filter(|key| !pressed(key))
            } else {
                None
            };
            let Some(help) = help else {
                break;
            };
            self.press(at, help);
        }
        for fresh in [false, true] {
            if fresh {
                if !self.driver.fresh() {
                    self.at = None;
                    return false;
                }
                self.driver.go(&[], None);
            } else {
                self.driver.home();
            }
            let seen = self.driver.observe();
            // A home screen not seen before (what was pressed changed
            // it) is where home leads now.
            self.at = Some(self.note(&seen, Vec::new()));
            if self.follow(to) {
                return true;
            }
        }
        false
    }

    /// Goes to place `to` along the presses learnt, from where the device
    /// is; anew from where one led elsewhere this time (thrice at most).
    fn follow(&mut self, to: usize) -> bool {
        for _ in 0..3 {
            let Some(at) = self.at else {
                return false;
            };
            if at == to {
                return true;
            }
            let Some(path) = self.path(at, to) else {
                return false;
            };
            let mut expected = at;
            for key in path {
                let leads = self.places[expected]
                    .moves
                    .iter()
                    .find(|(pressed, _)| *pressed == key)
                    .map(|&(_, to)| to);
                self.press(expected, key);
                let Some(leads) = leads.filter(|&leads| self.at == Some(leads)) else {
                    break;
                };
                expected = leads;
            }
        }
        self.at == Some(to)
    }

    /// Ends the search: for every screen of `wanted` seen, the shortest
    /// way learnt that leads to it again on a device as at the start.
    fn finish(mut self, wanted: &HashSet<String>) -> Found {
        self.found.views = self.views.len();
        self.found.places = self.places.len();
        self.found.steps = self.driver.steps - self.began;
        let mut screens: Vec<String> = self.found.ways.keys().cloned().collect();
        screens.retain(|screen| wanted.contains(screen));
        screens.sort();
        for screen in screens {
            // The ways to its places, the shortest first: along the
            // presses learnt, and as each was first come by. The device
            // asleep is no place.
            let mut ways: Vec<Vec<String>> = Vec::new();
            for (place, known) in self.places.iter().enumerate() {
                if known.seen.top() != screen {
                    continue;
                }
                if let Some(path) = self.path(0, place) {
                    ways.push(path.iter().map(|&key| self.keys[key].clone()).collect());
                }
                ways.push(known.way.clone());
            }
            ways.sort_by_key(Vec::len);
            ways.dedup();
            for way in ways.into_iter().take(3) {
                if !self.driver.fresh() || self.driver.go(&[], None).is_none() {
                    break;
                }
                for step in &way {
                    self.driver.step(step);
                }
                let seen = self.driver.observe();
                if seen.top() == screen {
                    self.found.ways.insert(screen.clone(), (way, seen.state()));
                    break;
                }
            }
        }
        self.found
    }
}
