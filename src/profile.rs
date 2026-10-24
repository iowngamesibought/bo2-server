//! Local client data, kept outside the publication tree.
use crate::{
    league_config::LeagueDef,
    public_config::{CategoryDef, PlaylistDef},
};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema_version: u32,
    pub notice: String,
    pub bundle_prefix: String,
    pub bundle_suffix: String,
    pub playlist_member_name: String,
    pub league_member_name: String,
    pub league_category_filter: String,
    pub minimum_player_settings: Vec<String>,
    pub maps: Vec<String>,
    pub leagues: Vec<LeagueDef>,
    pub categories: Vec<CategoryDef>,
    pub playlists: Vec<PlaylistDef>,
}

impl Profile {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if std::fs::metadata(path)?.len() > 1_000_000 {
            return Err("profile exceeds size limit".into());
        }
        let profile: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 127
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
        };
        let label = |s: &str| {
            !s.is_empty()
                && s.len() <= 255
                && !s.chars().any(|c| c.is_control() || c == '"' || c == '\\')
        };
        if self.schema_version != 1
            || self.leagues.is_empty()
            || self.leagues.len() > 16
            || self.maps.is_empty()
            || self.maps.len() > 128
            || self.categories.len() > 64
            || self.playlists.len() > 198
            || self.minimum_player_settings.is_empty()
            || self.minimum_player_settings.len() > 8
        {
            return Err("invalid profile schema or collection bounds");
        }
        for s in [
            &self.bundle_prefix,
            &self.bundle_suffix,
            &self.playlist_member_name,
            &self.league_member_name,
            &self.league_category_filter,
        ]
        .into_iter()
        .chain(self.maps.iter())
        .chain(self.minimum_player_settings.iter())
        {
            if !token(s) {
                return Err("invalid profile token");
            }
        }
        if self.playlist_member_name.len() > 31
            || self.league_member_name.len() > 31
            || self.playlist_member_name == self.league_member_name
        {
            return Err("invalid bundle member names");
        }
        let mut league_ids = BTreeSet::new();
        let mut playlist_ids = BTreeSet::new();
        let mut category_keys = BTreeSet::new();
        for l in &self.leagues {
            if l.id <= 0
                || !league_ids.insert(l.id)
                || l.playlist_id > 199
                || !playlist_ids.insert(l.playlist_id)
                || !token(&l.category)
                || !category_keys.insert(l.category.as_str())
                || !label(&l.name)
                || !label(&l.description)
                || !(1..=18).contains(&l.team_size)
                || l.placement_games > 100
                || l.gametypes.is_empty()
                || l.gametypes.len() > 32
                || l.gametypes.iter().any(|s| !token(s))
            {
                return Err("invalid or duplicate league definition");
            }
        }
        for c in &self.categories {
            if !token(&c.key)
                || !category_keys.insert(&c.key)
                || !label(&c.name)
                || !label(&c.description)
            {
                return Err("invalid or duplicate public category");
            }
        }
        for p in &self.playlists {
            if p.id > 199
                || !playlist_ids.insert(p.id)
                || !label(&p.name)
                || !label(&p.description)
                || !(1..=18).contains(&p.max_party)
                || !self.categories.iter().any(|c| c.key == p.category)
                || p.gametypes.is_empty()
                || p.gametypes.len() > 32
                || p.gametypes.iter().any(|s| !token(s))
            {
                return Err("invalid or duplicate public playlist");
            }
        }
        Ok(())
    }

    pub fn is_bundle_name(&self, name: &str) -> bool {
        name.starts_with(&self.bundle_prefix) && name.ends_with(&self.bundle_suffix)
    }
}

/// In-memory publisher response, prepared from a validated local profile.
#[derive(Clone, Debug, Default)]
pub struct PublisherAssets {
    pub bundle: Vec<u8>,
    pub prefix: String,
    pub suffix: String,
}

impl PublisherAssets {
    pub fn from_profile(
        profile: &Profile,
        timestamp: u32,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        profile.validate()?;
        Ok(Self {
            bundle: crate::league_config::online_wad(profile, timestamp)?,
            prefix: profile.bundle_prefix.clone(),
            suffix: profile.bundle_suffix.clone(),
        })
    }
    pub fn is_bundle_name(&self, name: &str) -> bool {
        !self.prefix.is_empty()
            && !self.suffix.is_empty()
            && name.starts_with(&self.prefix)
            && name.ends_with(&self.suffix)
    }
}
