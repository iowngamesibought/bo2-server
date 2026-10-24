//! Render operator-supplied public categories and playlists.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct CategoryDef {
    pub key: String,
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistDef {
    pub id: u32,
    pub name: String,
    pub description: String,
    pub category: String,
    pub max_party: u32,
    pub gametypes: Vec<String>,
}

/// The public `category` sections. They must come before every `playlist`
/// section: inside a playlist, `category <key>` assigns that playlist's
/// category instead of opening a new one (original loader, emulated).
pub fn render_categories(out: &mut String, categories: &[CategoryDef]) {
    for c in categories {
        out.push_str(&format!(
            "category {}\nname english \"{}\"\ndescription english \"{}\"\n",
            c.key, c.name, c.description
        ));
    }
}

/// The public `playlist` sections with their rotation entries.
pub fn render_playlists(out: &mut String, maps: &[String], playlists: &[PlaylistDef]) {
    for p in playlists {
        out.push_str(&format!(
            concat!(
                "playlist {}\nname english \"{}\"\ndescription english \"{}\"\n",
                "category {}\nranked\nminparty 1\nmaxparty {}\n"
            ),
            p.id, p.name, p.description, p.category, p.max_party
        ));
        for map in maps {
            for gametype in &p.gametypes {
                out.push_str(&format!("{map},{gametype},1\n"));
            }
        }
    }
}
