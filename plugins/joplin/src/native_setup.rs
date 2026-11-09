//! The rows of the set-up screen and what each is for (interface 6's
//! `setup.rs`, without the drawing): where the notes go, the account, the
//! notebook and *Send now*. The screen itself is the plugin's (`core.ui`).
use crate::native_joplin::Service;

/// A row of the set-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    Service,
    Address,
    User,
    Password,
    Notebook,
    Send,
}

const ALL: [Row; 6] = [
    Row::Service,
    Row::Address,
    Row::User,
    Row::Password,
    Row::Notebook,
    Row::Send,
];
/// Joplin Cloud has its address.
const CLOUD: [Row; 5] = [
    Row::Service,
    Row::User,
    Row::Password,
    Row::Notebook,
    Row::Send,
];

impl Row {
    /// The rows for `service`.
    #[must_use]
    pub fn of(service: Service) -> &'static [Row] {
        if service.has_address() { &ALL } else { &CLOUD }
    }

    /// Its title's key.
    #[must_use]
    pub const fn title(self, service: Service) -> &'static str {
        match self {
            Row::Service => "joplin.service",
            Row::Address => "joplin.address",
            Row::User if service.signs_in() => "joplin.email",
            Row::User => "joplin.user",
            Row::Password => "joplin.password",
            Row::Notebook => "joplin.notebook",
            Row::Send => "joplin.send",
        }
    }

    /// The key of what it is for.
    #[must_use]
    pub const fn help(self) -> &'static str {
        match self {
            Row::Service => "joplin.service.help",
            Row::Address => "joplin.address.help",
            Row::User => "joplin.user.help",
            Row::Password => "joplin.password.help",
            Row::Notebook => "joplin.notebook.help",
            Row::Send => "joplin.send.help",
        }
    }

    /// Its text setting, for those typed in.
    #[must_use]
    pub const fn setting(self) -> Option<&'static str> {
        match self {
            Row::Address => Some("joplin.address"),
            Row::User => Some("joplin.user"),
            Row::Notebook => Some("joplin.notebook"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_by_service() {
        assert!(!Row::of(Service::Cloud).contains(&Row::Address));
        assert!(Row::of(Service::WebDav).contains(&Row::Address));
        assert_eq!(Row::of(Service::Server).len(), 6);
        assert_eq!(Row::of(Service::Cloud).len(), 5);
        assert_eq!(Row::User.title(Service::Server), "joplin.email");
        assert_eq!(Row::User.title(Service::Cloud), "joplin.email");
        assert_eq!(Row::User.title(Service::WebDav), "joplin.user");
        // Typed rows have a setting, the password and the others none.
        assert_eq!(Row::Notebook.setting(), Some("joplin.notebook"));
        assert_eq!(Row::Password.setting(), None);
        assert_eq!(Row::Service.setting(), None);
        assert_eq!(Row::Send.setting(), None);
    }
}
