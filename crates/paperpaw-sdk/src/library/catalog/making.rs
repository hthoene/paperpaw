//! Making the catalog: walked anew from the library folder ([`Walk`]) or
//! caught up ([`Rewrite`]), written to a building file a step at a time
//! and put in the old one's place. A walk that finds the catalog as it
//! was writes nothing.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::library::self;
use crate::{Env, Storage, StorageError, files};
use paperpaw_digest::fnv1a64;






















