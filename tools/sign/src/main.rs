//! `paperpaw-sign`: signs firmware images as update files (see
//! `paperpaw-update`), makes signing keys and shows what an update file
//! holds; makes native plugin files (`.ppn`, see `paperpaw-native`) from
//! linked plugins, signs them as their author and shows what they hold;
//! checks that crates keep no mutable statics, as a plugin's copy of
//! the SDK must not; and compiles translation catalogs to be read from
//! the card as needed (`.ppcat`, ADR 0033).
//!
//! A device installs updates signed with PaperPaw's key, and those signed
//! with a key its reader chose to trust (their own builds). A plugin file
//! is signed by its author, whose key a device remembers at the first
//! install.

use std::process::ExitCode;

mod asset;
mod elf;
mod firmware;
mod frames;
mod hot;
mod image;
mod pack;
mod pack5;
mod plugin;
mod relocation;
mod statics;

const USAGE: &str = "\
paperpaw-sign: sign PaperPaw firmware updates, make and sign native plugins

USAGE:
    paperpaw-sign key <secret-key-file>
        Makes a new signing key, keeps it in the file (keep it safe) and
        prints its public key.
    paperpaw-sign sign <image.bin> <update.bin> [--key <secret-key-file>]
        Signs an app image (`espflash save-image`, not merged) as an update
        file. Without --key, the key comes from PAPERPAW_SIGNING_KEY (64
        hexadecimal digits).
    paperpaw-sign show <update.bin>
        Shows what the update holds, its signer, and whether its signature
        holds.
    paperpaw-sign sign-apart <flash.bin> <flash.bin.sig> [--key <secret-key-file>]
        Signs a whole flash image (as the web flasher writes it) with a
        signature file of its own; the key as for sign.
    paperpaw-sign check <flash.bin> <flash.bin.sig>
        Shows the signer of a whole flash image and whether its signature
        holds.
    paperpaw-sign sign-asset <file> <file.sig> [--key <secret-key-file>]
        Signs a release asset (an emulator, the handbook) with a signature
        file of its own, over \"paperpaw-asset-v1\" and a line end before the
        file: it never holds as a firmware's; the key as for sign.
    paperpaw-sign check-asset <file> <file.sig>
        Shows the signer of a release asset and whether its signature
        holds.
    paperpaw-sign check-image <flash.bin>
        Checks that a whole flash image takes updates over the air once
        flashed: its partition table has the OTA data and two app slots
        (read as the firmware reads it), its app has one data segment, and
        the app fits either slot and CrossPoint's. The release runs it on
        every device's image.
    paperpaw-sign plugins <image.bin>
        Lists the plugin crates an app image or update file has built in;
        fails if it lists none.
    paperpaw-sign fingerprint <firmware.elf>
        Prints the fingerprint native plugins must have to run on a
        firmware (16 hexadecimal digits), as its ELF file names it.
    paperpaw-sign abi <firmware.elf>
        Prints a release's -abi.txt for a firmware: its fingerprint, then
        `interface:`, `api:` (the size of its table for plugins),
        `families:` and `min:` (each family's revision and the oldest it
        runs, eight numbers), as its ELF file says them.
    paperpaw-sign pack <plugin.elf> <plugin.ppn> [--key <secret-key-file>]
        Makes a native plugin file from a plugin linked with
        paperpaw-plugin.x and --emit-relocs, or says what keeps it from
        moving (writable data, undefined symbols, addresses built into
        instructions, relative reaches out of the plugin). With --key,
        signs it as sign-plugin does. A plugin linked with
        paperpaw-plugin7.x (it defines PAPERPAW_METADATA) is an interface-7
        plugin and gets the file of format 5 (abi/ppn5.md), read back as
        the X4's loader reads it, with the same refusals for the X4 Pro
        (target 9, refused until an Xtensa profile exists), a symbol
        exported besides the entry and the metadata, and more than 8192
        patches. The plugin, sign-plugin and check-plugin commands read
        both formats.
    paperpaw-sign catalog <catalog.ini> <catalog.ppcat>
        Compiles a translation catalog to be read from the card as
        needed (ADR 0033): a data plugin's i18n/*.ppcat. Fails where it
        is no catalog, or a key or message is too long.
    paperpaw-sign rotation <plugin-id> <old-key> <new-key> [--key <recovery-key-file>]
        Signs the statement that moves a plugin from its author's key
        <old-key> to <new-key> (public keys, 64 hexadecimal digits each)
        with the author's recovery key, chosen in advance and kept
        offline; the key as for sign. Prints the signature (128
        hexadecimal digits) the store hands readers with the update.
    paperpaw-sign revocation <plugin-id> <key> [--key <secret-key-file>]
        Signs the statement that revokes <key> (a public key, 64
        hexadecimal digits), the key of the plugin's author: with that key
        itself, or with the author's recovery key where it was lost; the
        key as for sign. Prints the signature (128 hexadecimal digits) the
        store's library lists with the plugin and the key.
    paperpaw-sign plugin <plugin.ppn>
        Shows what a plugin file holds, and its author if it is signed
        (failing if the signature does not hold).
    paperpaw-sign sign-plugin <plugin.ppn> [--key <secret-key-file>]
        Signs a plugin file as its author: the signature goes at its end
        (one it had is replaced), and the author's key (64 hexadecimal
        digits) is printed, as the plugin library names it; the key as for
        sign. The signature is paperpaw-native's: Ed25519 over \"PaperPaw
        plugin file\", a zero byte, the file and the trailer's start.
    paperpaw-sign check-plugin <plugin.ppn>
        Shows the author of a plugin file and whether the signature holds;
        fails if it is not signed.
    paperpaw-sign check-publish [repo] [--release-dir <dir>] [--tag <tag>] [--rules <file>]
        Checks a plugin's repository by the rules of the plugin store at
        paperpaw.org, and with --release-dir the files of its release (their
        names, each build's chip, interface and signature); fails where the
        site would refuse it, and says what to change. --rules takes the
        site's newest rules (https://paperpaw.org/api/v1/store-rules.json).
        paperpaw-dev check-publish is the same check.
    paperpaw-sign store-rules [file]
        Prints the store's rules this tool checks by; with a file (the
        site's), fails where they differ.
    paperpaw-sign statics <crate.rlib>...
        Lists the symbols in writable memory (mutable statics) of the
        objects in rlibs (built without LTO); fails if there is one. A
        native plugin has no writable data, so the SDK's crates must keep
        none.
";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["key", file] => firmware::key(file),
        ["sign", image, update] => firmware::sign(image, update, None),
        ["sign", image, update, "--key", file] => firmware::sign(image, update, Some(file)),
        ["show", update] => firmware::show(update),
        ["sign-apart", image, signature] => firmware::sign_apart(image, signature, None),
        ["sign-apart", image, signature, "--key", file] => {
            firmware::sign_apart(image, signature, Some(file))
        }
        ["check", image, signature] => firmware::check(image, signature),
        ["sign-asset", file, signature] => asset::sign(file, signature, None),
        ["sign-asset", file, signature, "--key", key] => asset::sign(file, signature, Some(key)),
        ["check-asset", file, signature] => asset::check(file, signature),
        ["check-image", flash] => image::check_file(flash),
        ["plugins", image] => firmware::plugins(image),
        ["fingerprint", elf] => firmware::fingerprint(elf),
        ["abi", elf] => firmware::abi(elf),
        ["pack", elf, out] => plugin::pack(elf, out, None),
        ["pack", elf, out, "--key", key] => plugin::pack(elf, out, Some(key)),
        ["catalog", source, out] => catalog(source, out),
        ["rotation", plugin, old, new] => rotation(plugin, old, new, None),
        ["rotation", plugin, old, new, "--key", key] => rotation(plugin, old, new, Some(key)),
        ["revocation", plugin, key] => revocation(plugin, key, None),
        ["revocation", plugin, key, "--key", file] => revocation(plugin, key, Some(file)),
        ["plugin", file] => plugin::show(file),
        ["sign-plugin", file] => plugin::sign(file, None),
        ["sign-plugin", file, "--key", key] => plugin::sign(file, Some(key)),
        ["check-plugin", file] => plugin::check(file),
        ["statics", archives @ ..] if !archives.is_empty() => plugin::statics(archives),
        ["check-publish", rest @ ..] => check_publish(rest),
        ["store-rules"] => {
            print!("{}", paperpaw_publish::rules::BUILT_IN);
            Ok(())
        }
        ["store-rules", file] => store_rules(file),
        ["-h" | "--help"] => {
            print!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unexpected arguments\n\n{USAGE}")),
    }
}

/// `catalog`: the catalog `source` compiled into `out`.
fn catalog(source: &str, out: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(source).map_err(|e| format!("{source}: {e}"))?;
    let compiled = paperpaw_i18n::Catalog::compile(&text).map_err(|e| format!("{source}: {e}"))?;
    std::fs::write(out, &compiled).map_err(|e| format!("{out}: {e}"))?;
    println!("{out}: {} bytes", compiled.len());
    Ok(())
}

/// `rotation`: the recovery key's signature over the rotation of
/// `plugin` from `old` to `new`.
fn rotation(plugin: &str, old: &str, new: &str, key_file: Option<&str>) -> Result<(), String> {
    let public = |text: &str| -> Result<[u8; 32], String> {
        paperpaw_digest::hex::decode(text.trim())
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| format!("`{text}` is not a public key of 64 hexadecimal digits"))
    };
    let (old, new) = (public(old)?, public(new)?);
    if old == new {
        return Err("the old and the new key are the same".into());
    }
    let seed = secret_key(key_file)?;
    let message = paperpaw_native::signature::rotation_message(plugin, &old, &new);
    let signature = paperpaw_native::signature::sign_message(message.as_bytes(), &seed);
    println!("{}", paperpaw_digest::hex::encode(&signature));
    Ok(())
}

/// `revocation`: the signature over the revocation of `key` for
/// `plugin`, by the key in `key_file` (the revoked one, or the recovery
/// key).
fn revocation(plugin: &str, key: &str, key_file: Option<&str>) -> Result<(), String> {
    let key: [u8; 32] = paperpaw_digest::hex::decode(key.trim())
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| format!("`{key}` is not a public key of 64 hexadecimal digits"))?;
    let seed = secret_key(key_file)?;
    let message = paperpaw_native::signature::revocation_message(plugin, &key);
    let signature = paperpaw_native::signature::sign_message(message.as_bytes(), &seed);
    println!("{}", paperpaw_digest::hex::encode(&signature));
    Ok(())
}

/// `check-publish`: the problems, one a line; fails on an error.
fn check_publish(args: &[&str]) -> Result<(), String> {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    let problems = paperpaw_publish::run(&args, None)?;
    for problem in &problems {
        println!("{problem}");
    }
    if paperpaw_publish::passes(&problems) {
        println!("check-publish: the plugin store takes it");
        Ok(())
    } else {
        Err("the plugin store would refuse it: change what the errors say".into())
    }
}

/// `store-rules <file>`: whether the rules in `file` are this tool's.
fn store_rules(file: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
    let theirs = paperpaw_publish::Rules::parse(&text).map_err(|e| format!("{file}: {e}"))?;
    if theirs == paperpaw_publish::Rules::built_in() {
        println!("the same rules");
        Ok(())
    } else {
        Err(format!(
            "{file} has other rules than tools/publish/store-rules.json: bring them in step"
        ))
    }
}

/// The secret key from `key_file`, else from PAPERPAW_SIGNING_KEY.
fn secret_key(key_file: Option<&str>) -> Result<[u8; 32], String> {
    let text = match key_file {
        Some(file) => std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?,
        None => std::env::var("PAPERPAW_SIGNING_KEY")
            .map_err(|_| "no --key and no PAPERPAW_SIGNING_KEY".to_string())?,
    };
    Ok(paperpaw_update::parse_key(&text).ok_or("the key is not 64 hexadecimal digits")?)
}
