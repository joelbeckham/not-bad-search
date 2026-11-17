use not_bad_search::{slack_display as display,Hit,sources::Source};
#[test]
fn emoji_aliases_preserve_unknown_codes_urls_and_code() {
 assert_eq!(display::emojis(":date: Coming up :e-mail: Engage"),"📅 Coming up 📧 Engage");
 assert_eq!(display::emojis(":+1::skin-tone-3: :party_parrot:"),"👍🏼 :party_parrot:");
 assert_eq!(display::emojis("`keep :date:` https://example.com/:date: :date:"),"`keep :date:` https://example.com/:date: 📅");
 assert_eq!(display::emojis("```\n:date:\n```\n:date:"),"```\n:date:\n```\n📅");
 assert_eq!(display::emojis("é :unknown::date: 10:08"),"é :unknown:📅 10:08");
}
#[test]
fn cached_titles_previews_and_attachments_use_source_name() {
 let mut source=Source::slack_source();source.id="source".into();source.name="engineering-updates".into();
 let settings=source.slack.as_mut().unwrap();settings.channel="C12345678".into();settings.workspace="https://example.slack.com".into();
 let sources=vec![source];
 let mut hit=Hit{path:"not-bad-search://slack/source/C12345678/123.456789".into(),name:"C12345678 · :date: Coming up".into(),snippet:Some(":e-mail: Engage".into()),original_url:Some("https://example.slack.com/archives/C12345678/p123456789".into()),jira:None,gmail:None,notion:false,slack:true,slack_replies:None,timestamp:None};
 display::decorate(&mut hit,&sources);
 assert_eq!(hit.name,"#engineering-updates · 📅 Coming up");assert_eq!(hit.snippet.as_deref(),Some("📧 Engage"));
 assert_eq!(display::preview("C12345678\nFrom: Author\n:date: Coming up",Some("#engineering-updates")),"#engineering-updates\nFrom: Author\n📅 Coming up");
 hit.path="C:/cache/image.png".into();hit.name="screenshot.png · Slack".into();
 assert_eq!(display::channel_name(&hit,&sources).as_deref(),Some("#engineering-updates"));
 display::decorate(&mut hit,&sources);assert_eq!(hit.name,"screenshot.png · Slack");
 hit.slack=false;hit.name=":date: local file".into();display::decorate(&mut hit,&sources);assert_eq!(hit.name,":date: local file");
}

#[cfg(windows)]
#[test]
fn windows_emoji_font_renders_calendar_and_email_in_color() {
 use eframe::egui;
 let mut fonts=egui::FontDefinitions::default();
 display::add_emoji_font(&mut fonts);
 assert!(fonts.font_data.contains_key("Windows emoji"));
 let ctx=egui::Context::default();ctx.set_fonts(fonts);
 let mut output=ctx.run_ui(Default::default(),|ui| {ui.label(egui::RichText::new("📅 📧").size(24.));});
 assert!(output.textures_delta.set.iter().flat_map(|(_,deltas)|deltas).any(|delta| {let egui::ImageData::Color(image)=&delta.image;image.pixels.iter().any(|p|p.a()>0 && (p.r()!=p.g() || p.g()!=p.b()))}),"Expected colored emoji pixels in font atlas");
 output.textures_delta.clear();
}
