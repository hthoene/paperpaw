use super::*;

/// A folder of its own, removed at the end.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!("pp-host-seal-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn key_file(&self) -> PathBuf {
        self.0.join("device.key")
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const AAD: &[u8] = b"owner\0name";

fn seal(s: &FileSealer, aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, i32> {
    let mut out = vec![0u8; NONCE + plain.len() + TAG];
    let n = s.seal(aad, plain, &mut out)?;
    out.truncate(n);
    Ok(out)
}

fn open(s: &FileSealer, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, i32> {
    let mut out = vec![0u8; sealed.len()];
    let n = s.open(aad, sealed, &mut out)?;
    out.truncate(n);
    Ok(out)
}

#[test]
fn a_value_comes_back() {
    let dir = Dir::new("round");
    let s = FileSealer::new(dir.key_file());
    assert!(s.seals());
    let sealed = seal(&s, AAD, b"hunter2").unwrap();
    assert_eq!(sealed.len(), NONCE + 7 + TAG);
    assert!(!sealed.windows(7).any(|w| w == b"hunter2"));
    assert_eq!(open(&s, AAD, &sealed).unwrap(), b"hunter2");
    // The empty value has a tag too, and the longest one fits.
    assert_eq!(open(&s, AAD, &seal(&s, AAD, b"").unwrap()).unwrap(), b"");
    let long = vec![7u8; MAX_PLAIN];
    assert_eq!(open(&s, AAD, &seal(&s, AAD, &long).unwrap()).unwrap(), long);
    assert_eq!(seal(&s, AAD, &vec![0u8; MAX_PLAIN + 1]), Err(ERR_INVALID));
}

#[test]
fn the_same_value_seals_differently_each_time() {
    let dir = Dir::new("nonce");
    let s = FileSealer::new(dir.key_file());
    let a = seal(&s, AAD, b"same").unwrap();
    let b = seal(&s, AAD, b"same").unwrap();
    assert_ne!(a[..NONCE], b[..NONCE]);
    assert_ne!(a, b);
}

#[test]
fn owner_and_name_are_authenticated() {
    let dir = Dir::new("aad");
    let s = FileSealer::new(dir.key_file());
    let sealed = seal(&s, b"a\0pass", b"x").unwrap();
    // A value copied to another owner's line, or another name, is absent.
    assert_eq!(open(&s, b"b\0pass", &sealed), Err(ERR_NOT_FOUND));
    assert_eq!(open(&s, b"a\0other", &sealed), Err(ERR_NOT_FOUND));
    assert_eq!(open(&s, b"a\0pass", &sealed).unwrap(), b"x");
}

#[test]
fn a_changed_bit_is_absent_and_leaves_nothing() {
    let dir = Dir::new("tamper");
    let s = FileSealer::new(dir.key_file());
    let sealed = seal(&s, AAD, b"secret value").unwrap();
    for at in 0..sealed.len() {
        let mut bad = sealed.clone();
        bad[at] ^= 1;
        let mut out = vec![0xAAu8; bad.len()];
        assert_eq!(s.open(AAD, &bad, &mut out), Err(ERR_NOT_FOUND), "byte {at}");
        assert!(out.iter().all(|&b| b == 0 || b == 0xAA), "byte {at}");
        assert!(
            !out.windows(6).any(|w| w == b"secret"),
            "plaintext left at {at}"
        );
    }
    // Too short and too long are invalid, not a panic.
    assert_eq!(open(&s, AAD, &sealed[..NONCE + TAG - 1]), Err(ERR_INVALID));
    assert_eq!(
        open(&s, AAD, &vec![0u8; NONCE + MAX_PLAIN + TAG + 1]),
        Err(ERR_INVALID)
    );
}

#[test]
fn another_device_opens_nothing() {
    let (one, two) = (Dir::new("dev-one"), Dir::new("dev-two"));
    let (a, b) = (
        FileSealer::new(one.key_file()),
        FileSealer::new(two.key_file()),
    );
    let sealed = seal(&a, AAD, b"mine").unwrap();
    // The second device has no key yet: absent, and opening makes no key.
    assert_eq!(open(&b, AAD, &sealed), Err(ERR_NOT_FOUND));
    assert!(!two.key_file().exists());
    // With a key of its own it is absent still.
    seal(&b, AAD, b"yours").unwrap();
    assert_eq!(open(&b, AAD, &sealed), Err(ERR_NOT_FOUND));
}

#[test]
fn the_key_is_made_at_the_first_seal_and_kept() {
    let dir = Dir::new("keep");
    let s = FileSealer::new(dir.key_file());
    assert_eq!(open(&s, AAD, &[0u8; NONCE + TAG]), Err(ERR_NOT_FOUND));
    assert!(!dir.key_file().exists());
    let sealed = seal(&s, AAD, b"v").unwrap();
    let on_disk = fs::read(dir.key_file()).unwrap();
    assert_eq!(on_disk.len(), LEN);
    assert!(parse(&on_disk).is_some());
    // A new sealer on the same file (a restart) opens it, and the file is
    // not rewritten.
    let again = FileSealer::new(dir.key_file());
    assert_eq!(open(&again, AAD, &sealed).unwrap(), b"v");
    seal(&again, AAD, b"w").unwrap();
    assert_eq!(fs::read(dir.key_file()).unwrap(), on_disk);
    assert!(!dir.0.join("device.key.new").exists());
}

#[cfg(unix)]
#[test]
fn the_key_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Dir::new("mode");
    let s = FileSealer::new(dir.key_file());
    seal(&s, AAD, b"v").unwrap();
    let mode = fs::metadata(dir.key_file()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_damaged_key_file_is_absent_until_the_next_seal_which_makes_a_new_key() {
    let dir = Dir::new("damaged");
    let s = FileSealer::new(dir.key_file());
    let sealed = seal(&s, AAD, b"old").unwrap();
    let good = fs::read(dir.key_file()).unwrap();
    for bad in [
        vec![],
        vec![0xFF; LEN],
        good[..LEN - 1].to_vec(),
        [good.clone(), vec![0]].concat(),
        {
            let mut flipped = good.clone();
            flipped[20] ^= 1;
            flipped
        },
    ] {
        fs::write(dir.key_file(), &bad).unwrap();
        assert_eq!(open(&s, AAD, &sealed), Err(ERR_NOT_FOUND));
        // Opening never touches the file.
        assert_eq!(fs::read(dir.key_file()).unwrap(), bad);
    }
    // A seal makes a new key; what was sealed before stays absent.
    seal(&s, AAD, b"new").unwrap();
    assert_ne!(fs::read(dir.key_file()).unwrap(), good);
    assert_eq!(open(&s, AAD, &sealed), Err(ERR_NOT_FOUND));
}

#[test]
fn a_key_file_that_cannot_be_read_is_not_replaced() {
    let dir = Dir::new("unreadable");
    // A directory where the file should be: not "missing", so no new key.
    fs::create_dir(dir.key_file()).unwrap();
    let s = FileSealer::new(dir.key_file());
    assert_eq!(seal(&s, AAD, b"v"), Err(ERR_IO));
    assert!(dir.key_file().is_dir());
}

#[test]
fn no_folder_is_no_seal() {
    let dir = Dir::new("nofolder");
    let s = FileSealer::new(dir.0.join("missing").join("device.key"));
    // Nothing is written: the host refuses the save.
    assert_eq!(seal(&s, AAD, b"v"), Err(ERR_IO));
    assert!(!dir.0.join("missing").exists());
}

#[test]
fn the_output_must_fit() {
    let dir = Dir::new("room");
    let s = FileSealer::new(dir.key_file());
    let mut small = [0u8; NONCE + 3 + TAG - 1];
    assert_eq!(s.seal(AAD, b"abc", &mut small), Err(ERR_NO_SPACE));
    let sealed = seal(&s, AAD, b"abc").unwrap();
    let mut small = [0u8; 2];
    assert_eq!(s.open(AAD, &sealed, &mut small), Err(ERR_NO_SPACE));
}

#[test]
fn the_record_is_the_x4s_record() {
    // The same bytes as `paperpaw_x4::device_key::record` makes (magic, key,
    // CRC-32 little endian): pinned by value, since the crates do not depend on
    // each other.
    let key: [u8; KEY] = core::array::from_fn(|i| i as u8 ^ 0x5A);
    let record = record(&key);
    assert_eq!(&record[..8], b"PPDEVKY1");
    assert_eq!(&record[8..40], &key);
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(&record[40..], crc32(&record[..40]).to_le_bytes());
    assert_eq!(*parse(&record).unwrap(), key);
}

#[test]
fn os_random_is_strong_and_fills() {
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    assert_eq!(OsRandom.fill(&mut a), Ok(true));
    assert_eq!(OsRandom.fill(&mut b), Ok(true));
    assert_ne!(a, b);
}

#[test]
fn a_second_creator_never_replaces_the_first_key() {
    let dir = Dir::new("exclusive");
    let s = FileSealer::new(dir.key_file());
    let first = [1u8; KEY];
    let second = [2u8; KEY];
    s.write(&first, false).unwrap();
    // The file is there: the second write leaves it alone and is no error.
    s.write(&second, false).unwrap();
    assert_eq!(fs::read(dir.key_file()).unwrap(), record(&first));
}

#[test]
fn creators_at_once_end_with_one_key_that_all_use() {
    let dir = Dir::new("race");
    let path = dir.key_file();
    let sealed: Vec<Vec<u8>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let path = path.clone();
                scope.spawn(move || seal(&FileSealer::new(path), AAD, b"v").unwrap())
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let s = FileSealer::new(&path);
    for one in &sealed {
        assert_eq!(open(&s, AAD, one).unwrap(), b"v");
    }
}

#[test]
fn nothing_but_the_key_file_is_left_in_the_folder() {
    let dir = Dir::new("leftover");
    let s = FileSealer::new(dir.key_file());
    seal(&s, AAD, b"v").unwrap();
    // Also after a replaced damaged file.
    fs::write(dir.key_file(), b"junk").unwrap();
    seal(&s, AAD, b"v").unwrap();
    let names: Vec<_> = fs::read_dir(&dir.0)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["device.key"]);
}

#[test]
fn the_temporary_file_is_private_from_the_start() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Dir::new("tempmode");
    let s = FileSealer::new(dir.key_file());
    // Widest umask-free check: the key file made by hard link shares the
    // temporary file's inode, so its mode is the one the temporary file had.
    s.write(&[3u8; KEY], false).unwrap();
    let mode = fs::metadata(dir.key_file()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

/// Each way a key is published where no hard link is: the rename that never
/// replaces, and (where the system has none) the lock file.
fn no_link_sealers(path: &Path) -> [FileSealer; 2] {
    [
        FileSealer::new(path).without_hard_links(),
        FileSealer::new(path)
            .without_hard_links()
            .without_rename_new(),
    ]
}

#[test]
fn a_folder_with_no_hard_links_gets_a_key_and_a_second_creator_leaves_it() {
    for mode in 0..2 {
        let dir = Dir::new(&format!("nolinks{mode}"));
        let [a, b] = no_link_sealers(&dir.key_file());
        let s = if mode == 0 { &a } else { &b };
        let first = [1u8; KEY];
        s.write(&first, false).unwrap();
        assert_eq!(fs::read(dir.key_file()).unwrap(), record(&first));
        // The file is there: the second write leaves it alone and is no error.
        s.write(&[2u8; KEY], false).unwrap();
        assert_eq!(fs::read(dir.key_file()).unwrap(), record(&first));
        // A seal and an open work over it, and nothing is left beside the key.
        let sealed = seal(s, AAD, b"v").unwrap();
        assert_eq!(open(s, AAD, &sealed).unwrap(), b"v");
        let names: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["device.key"], "mode {mode}");
    }
}

/// Eight threads publish their own key at once, straight through `write` (not
/// through `key`, whose process-wide lock would make them one at a time): one
/// wins whole, the others leave it, nothing else is left behind.
#[test]
fn publishers_at_once_with_no_hard_links_leave_the_first_key_whole() {
    for mode in 0..2 {
        let dir = Dir::new(&format!("nolinks-race{mode}"));
        let path = dir.key_file();
        // What each thread read from the file right after its own `write`: a
        // key replaced later would show as a thread that saw another one.
        let seen: Vec<Vec<u8>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8u8)
                .map(|n| {
                    let path = path.clone();
                    scope.spawn(move || {
                        let [a, b] = no_link_sealers(&path);
                        let s = if mode == 0 { a } else { b };
                        s.write(&[n + 1; KEY], false).unwrap();
                        fs::read(&path).unwrap()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let found = fs::read(&path).unwrap();
        assert!(
            (1..=8u8).any(|n| found == record(&[n; KEY])),
            "one whole record, mode {mode}"
        );
        assert!(
            seen.iter().all(|one| *one == found),
            "every publisher saw the same key (none was replaced), mode {mode}"
        );
        let names: Vec<_> = fs::read_dir(&dir.0)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["device.key"], "mode {mode}");
    }
}

#[test]
fn a_leftover_lock_is_taken_away_and_a_fresh_one_is_waited_for() {
    let dir = Dir::new("nolinks-lock");
    let mut lock = dir.key_file().into_os_string();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    // A lock of a creator that was cut off long ago.
    let file = fs::File::create(&lock).unwrap();
    file.set_modified(std::time::SystemTime::now() - 2 * STALE)
        .unwrap();
    drop(file);
    let s = FileSealer::new(dir.key_file())
        .without_hard_links()
        .without_rename_new();
    s.write(&[5u8; KEY], false).unwrap();
    assert_eq!(fs::read(dir.key_file()).unwrap(), record(&[5u8; KEY]));
    assert!(!lock.exists(), "the lock is gone with its holder");
    // A fresh lock held by someone: waited for, then given up (an error, no key).
    fs::remove_file(dir.key_file()).unwrap();
    fs::File::create(&lock).unwrap();
    let started = std::time::Instant::now();
    assert!(s.write(&[6u8; KEY], false).is_err());
    assert!(started.elapsed() >= WAIT_STEP * (WAIT_STEPS - 1));
    assert!(!dir.key_file().exists(), "no key is made without the lock");
}

/// A lock that cannot be taken away (a folder of that name) ends the wait with
/// an error, old or fresh: it never loops for ever.
#[test]
fn a_lock_that_cannot_be_removed_ends_with_an_error() {
    let dir = Dir::new("nolinks-lockdir");
    let mut lock = dir.key_file().into_os_string();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    fs::create_dir(&lock).unwrap();
    let s = FileSealer::new(dir.key_file())
        .without_hard_links()
        .without_rename_new();
    // Fresh: waited for, then refused.
    assert!(s.write(&[7u8; KEY], false).is_err());
    // Old: its removal is refused at once, and that is the error.
    fs::File::open(&lock)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - 2 * STALE)
        .unwrap();
    let started = std::time::Instant::now();
    assert!(s.write(&[7u8; KEY], false).is_err());
    assert!(
        started.elapsed() < STALE,
        "ended by the refusal, not by waiting"
    );
    assert!(!dir.key_file().exists());
}

/// A rename that never replaces does not replace: the key that is there stays,
/// also when the lock of the last resort was broken under its holder.
#[test]
#[cfg(target_os = "linux")]
fn the_rename_that_never_replaces_leaves_a_file_that_is_there() {
    let dir = Dir::new("noreplace");
    let (from, to) = (dir.0.join("from"), dir.key_file());
    fs::write(&from, b"new").unwrap();
    fs::write(&to, b"old").unwrap();
    let e = rename_new(&from, &to).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(fs::read(&to).unwrap(), b"old");
    fs::remove_file(&to).unwrap();
    rename_new(&from, &to).unwrap();
    assert_eq!(fs::read(&to).unwrap(), b"new");
    assert!(!from.exists());
}

/// A temporary file left by an earlier power cut is not this call's: it is
/// neither deleted nor read as the key, whatever the outcome of a write.
#[test]
fn a_leftover_temporary_file_of_an_earlier_cut_is_left_alone() {
    let dir = Dir::new("leftover-temp");
    let mut name = dir.key_file().into_os_string();
    name.push(".new-1-1");
    let leftover = PathBuf::from(name);
    fs::write(&leftover, record(&[9u8; KEY])).unwrap();
    let s = FileSealer::new(dir.key_file());
    // No key yet: a seal makes its own, not the leftover's.
    let sealed = seal(&s, AAD, b"v").unwrap();
    assert_ne!(fs::read(dir.key_file()).unwrap(), record(&[9u8; KEY]));
    assert_eq!(open(&s, AAD, &sealed).unwrap(), b"v");
    assert_eq!(fs::read(&leftover).unwrap(), record(&[9u8; KEY]));
}
