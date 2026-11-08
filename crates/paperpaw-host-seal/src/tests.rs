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
