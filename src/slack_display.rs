//! Presentation only: keep original Slack text searchable and render cached results immediately.
use std::{collections::HashMap,sync::OnceLock};
use crate::{Hit,sources::Source};
/// Prefer the installed Windows color emoji font before broader system fallback.
pub fn add_emoji_font(fonts:&mut eframe::egui::FontDefinitions) {
    #[cfg(windows)]
    if let Some(windows)=std::env::var_os("WINDIR") {
        if let Ok(bytes)=std::fs::read(std::path::PathBuf::from(windows).join("Fonts/seguiemj.ttf")) {
            use eframe::egui::{FontData,FontFamily};
            fonts.font_data.insert("Windows emoji".into(),FontData::from_owned(bytes).into());
            for family in [FontFamily::Proportional,FontFamily::Monospace] {fonts.families.entry(family).or_default().push("Windows emoji".into());}
        }
    }
    #[cfg(not(windows))]
    let _=fonts;
}
fn aliases()->&'static HashMap<String,String> {
    static MAP:OnceLock<HashMap<String,String>>=OnceLock::new();
    MAP.get_or_init(|| {
        #[derive(serde::Deserialize)] struct Entry {emoji:String,aliases:Vec<String>}
        let entries:Vec<Entry>=serde_json::from_str(include_str!("../assets/emoji/gemoji.json")).expect("Bundled emoji mapping");
        let mut map=HashMap::new();
        for entry in entries {for alias in entry.aliases {map.insert(alias,entry.emoji.clone());}}
        for (alias,emoji) in [("simple_smile","🙂"),("slightly_smiling_face","🙂"),("slightly_frowning_face","🙁"),("skin-tone-2","🏻"),("skin-tone-3","🏼"),("skin-tone-4","🏽"),("skin-tone-5","🏾"),("skin-tone-6","🏿")] {map.insert(alias.into(),emoji.into());}
        map
    })
}
pub fn emojis(text:&str)->String {
    let mut output=String::with_capacity(text.len());
    let mut code=false;
    for token in text.split_inclusive(char::is_whitespace) {
        // Leave URL paths and their colon-containing parameters untouched.
        if !code && token.contains("://") && !token.contains('`') {output.push_str(token);continue;}
        let mut rest=token;
        while !rest.is_empty() {
            let ch=rest.chars().next().unwrap();
            if ch=='`' {code=!code;}
            if ch==':' && !code {
                if let Some(end)=rest[1..].find(':').map(|n|n+1) {
                    if end<=65 {
                        if let Some(emoji)=aliases().get(&rest[1..end]) {
                            output.push_str(emoji);rest=&rest[end+1..];continue;
                        }
                    }
                }
            }
            output.push(ch);rest=&rest[ch.len_utf8()..];
        }
    }
    output
}
pub fn channel_name(hit:&Hit,sources:&[Source])->Option<String> {
    if !hit.slack {return None;}
    sources.iter().find(|s| {
        if let Some(settings)=&s.slack {
            hit.path.starts_with(&format!("not-bad-search://slack/{}/",s.id)) || hit.original_url.as_deref().is_some_and(|url| url.starts_with(&format!("{}/archives/{}/",settings.workspace.trim_end_matches('/'),settings.channel)))
        } else {false}
    }).map(|s|format!("#{}",s.name.trim_start_matches('#')))
}
pub fn decorate(hit:&mut Hit,sources:&[Source]) {
    if !hit.slack {return;}
    if hit.path.starts_with("not-bad-search://slack/") {
        if let Some((_,body))=hit.name.split_once(" · ") {
            hit.name=match channel_name(hit,sources) {Some(name)=>format!("{name} · {body}"),None=>body.to_owned()};
        }
    }
    hit.name=emojis(&hit.name);
    if let Some(snippet)=&mut hit.snippet {*snippet=emojis(snippet);}
}
pub fn preview(text:&str,channel:Option<&str>)->String {
    let body=text.split_once('\n').map_or(text,|(_,body)|body);
    emojis(&format!("{}\n{body}",channel.unwrap_or("Slack")))
}
