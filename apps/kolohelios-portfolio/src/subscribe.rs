//! Pure, host-testable logic for the `/api/subscribe` endpoint: input
//! validation, honeypot/spam rejection, and Kit v3 payload construction.
//!
//! Nothing here touches `worker::` types, so `cargo test` exercises it on
//! the host even though the crate otherwise compiles to wasm for the
//! Worker runtime. The thin `worker`-typed glue (reading the form body,
//! making the subrequest, shaping the HTTP response) lives in `lib.rs`
//! and calls into these functions.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

/// Form field names shared between the `contact.html` templates and the
/// worker. The hidden `kind` field selects which Kit form a submission
/// targets; `hp_url` is the honeypot.
pub const FIELD_KIND: &str = "kind";
pub const FIELD_EMAIL: &str = "email";
pub const FIELD_FIRST_NAME: &str = "first_name";
pub const FIELD_MESSAGE: &str = "message";
pub const FIELD_HONEYPOT: &str = "hp_url";

/// Which Kit form a submission targets. Contact and newsletter map to
/// distinct Kit form ids (resolved from env in `lib.rs`), so subscribers
/// from the two forms stay distinguishable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Contact,
    Newsletter,
}

impl Kind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "contact" => Some(Self::Contact),
            "newsletter" => Some(Self::Newsletter),
            _ => None,
        }
    }
}

/// A validated submission, ready to turn into a Kit payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub kind: Kind,
    pub email: String,
    pub first_name: Option<String>,
    pub message: Option<String>,
}

/// Why a raw submission was rejected before any Kit call. `Honeypot` is
/// handled specially by the caller (answered with a success-looking
/// response so bots get no signal); the rest surface as a 400.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    Honeypot,
    MissingKind,
    InvalidEmail,
    MissingMessage,
}

/// Validate a raw field map already extracted from the form body.
pub fn validate(fields: &HashMap<String, String>) -> Result<Submission, Rejection> {
    // Honeypot: a non-empty hidden field means a bot filled it in.
    if fields
        .get(FIELD_HONEYPOT)
        .is_some_and(|v| !v.trim().is_empty())
    {
        return Err(Rejection::Honeypot);
    }

    let kind = fields
        .get(FIELD_KIND)
        .and_then(|s| Kind::parse(s.trim()))
        .ok_or(Rejection::MissingKind)?;

    let email = fields.get(FIELD_EMAIL).map(|s| s.trim()).unwrap_or("");
    if !is_valid_email(email) {
        return Err(Rejection::InvalidEmail);
    }

    let first_name = non_empty(fields.get(FIELD_FIRST_NAME));
    let message = non_empty(fields.get(FIELD_MESSAGE));

    // A contact submission with no message is meaningless — reject it
    // rather than create an empty subscriber.
    if kind == Kind::Contact && message.is_none() {
        return Err(Rejection::MissingMessage);
    }

    Ok(Submission {
        kind,
        email: email.to_string(),
        first_name,
        message,
    })
}

fn non_empty(v: Option<&String>) -> Option<String> {
    v.map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Minimal, dependency-free email sanity check: exactly one `@`, a
/// non-empty local part, and a dotted domain. Kit does the authoritative
/// verification via double opt-in; this only filters obvious garbage
/// before spending a subrequest.
pub fn is_valid_email(email: &str) -> bool {
    let email = email.trim();
    if email.is_empty() || email.len() > 254 || email.contains(char::is_whitespace) {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
}

/// The JSON body Kit's v3 `forms/{form_id}/subscribe` endpoint expects.
/// A contact `message` is sent as a custom field (`fields[message]`),
/// which requires a matching custom field configured in the Kit account.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct KitPayload {
    pub api_key: String,
    pub email: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

/// Build the Kit request body for a validated submission.
pub fn kit_payload(sub: &Submission, api_key: &str) -> KitPayload {
    let mut fields = BTreeMap::new();
    if let Some(message) = &sub.message {
        fields.insert("message".to_string(), message.clone());
    }
    KitPayload {
        api_key: api_key.to_string(),
        email: sub.email.clone(),
        first_name: sub.first_name.clone(),
        fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert(FIELD_KIND.into(), "newsletter".into());
        m.insert(FIELD_EMAIL.into(), "person@example.com".into());
        m
    }

    #[test]
    fn accepts_a_valid_newsletter_submission() {
        let sub = validate(&base()).expect("valid");
        assert_eq!(sub.kind, Kind::Newsletter);
        assert_eq!(sub.email, "person@example.com");
        assert_eq!(sub.first_name, None);
        assert_eq!(sub.message, None);
    }

    #[test]
    fn accepts_a_contact_submission_with_a_message() {
        let mut m = base();
        m.insert(FIELD_KIND.into(), "contact".into());
        m.insert(FIELD_FIRST_NAME.into(), "  Ada  ".into());
        m.insert(FIELD_MESSAGE.into(), "  hello there  ".into());
        let sub = validate(&m).expect("valid");
        assert_eq!(sub.kind, Kind::Contact);
        assert_eq!(sub.first_name.as_deref(), Some("Ada"));
        assert_eq!(sub.message.as_deref(), Some("hello there"));
    }

    #[test]
    fn rejects_a_tripped_honeypot() {
        let mut m = base();
        m.insert(FIELD_HONEYPOT.into(), "http://spam.example".into());
        assert_eq!(validate(&m), Err(Rejection::Honeypot));
    }

    #[test]
    fn empty_honeypot_is_not_a_trip() {
        let mut m = base();
        m.insert(FIELD_HONEYPOT.into(), "   ".into());
        assert!(validate(&m).is_ok());
    }

    #[test]
    fn rejects_an_unknown_kind() {
        let mut m = base();
        m.insert(FIELD_KIND.into(), "spam".into());
        assert_eq!(validate(&m), Err(Rejection::MissingKind));
    }

    #[test]
    fn rejects_contact_without_a_message() {
        let mut m = base();
        m.insert(FIELD_KIND.into(), "contact".into());
        assert_eq!(validate(&m), Err(Rejection::MissingMessage));
    }

    #[test]
    fn rejects_garbage_emails() {
        for bad in [
            "",
            "no-at-sign",
            "@example.com",
            "a@b",
            "a@.com",
            "a@b.",
            "a b@example.com",
            "two@@example.com",
        ] {
            assert!(!is_valid_email(bad), "{bad:?} should be invalid");
        }
    }

    #[test]
    fn accepts_reasonable_emails() {
        for ok in ["a@b.co", "first.last@sub.example.com", "x+tag@example.org"] {
            assert!(is_valid_email(ok), "{ok:?} should be valid");
        }
    }

    #[test]
    fn payload_includes_message_as_a_custom_field() {
        let sub = Submission {
            kind: Kind::Contact,
            email: "a@b.co".into(),
            first_name: Some("Ada".into()),
            message: Some("hi".into()),
        };
        let payload = kit_payload(&sub, "secret-key");
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["api_key"], "secret-key");
        assert_eq!(json["email"], "a@b.co");
        assert_eq!(json["first_name"], "Ada");
        assert_eq!(json["fields"]["message"], "hi");
    }

    #[test]
    fn payload_omits_empty_optional_fields() {
        let sub = Submission {
            kind: Kind::Newsletter,
            email: "a@b.co".into(),
            first_name: None,
            message: None,
        };
        let json = serde_json::to_value(kit_payload(&sub, "k")).unwrap();
        assert!(
            json.get("first_name").is_none(),
            "first_name should be omitted"
        );
        assert!(
            json.get("fields").is_none(),
            "empty fields should be omitted"
        );
    }
}
