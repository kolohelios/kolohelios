use std::fmt;
use std::str::FromStr;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Surface a post is destined for. Mirrors LinkedIn's split: `post` is
/// short-form feed content; `article` is long-form with its own
/// permanent URL. Drives prompt selection, exit-criteria thresholds,
/// and optional model overrides per stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    #[default]
    Post,
    Article,
}

impl Kind {
    pub const ALL: &'static [Kind] = &[Kind::Post, Kind::Article];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Post => "post",
            Kind::Article => "article",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Kind {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "post" => Ok(Kind::Post),
            "article" => Ok(Kind::Article),
            other => Err(Error::InvalidKind(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_round_trips_for_each_kind() {
        for &k in Kind::ALL {
            assert_eq!(k.as_str().parse::<Kind>().unwrap(), k);
        }
    }

    #[test]
    fn parse_rejects_unknown_kind() {
        let err = "essay".parse::<Kind>().unwrap_err();
        assert!(matches!(err, Error::InvalidKind(_)));
    }

    #[test]
    fn default_is_post() {
        assert_eq!(Kind::default(), Kind::Post);
    }
}
