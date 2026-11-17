use not_bad_search::preview::{load, Content, TEXT_LIMIT};

#[test]
fn text_preview_matches_unicode_phrases_and_bounds() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("notes.md");
    std::fs::write(
        &path,
        "\u{feff}# Notes\nNo match\nCAFÉ and project budget with İ\n",
    )?;
    let Content::Text {
        lines,
        first_match,
        truncated,
    } = load(&path, "café \"project budget\" i")?
    else {
        panic!("expected text")
    };
    assert_eq!(first_match, 2);
    assert!(!truncated);
    let matched: Vec<_> = lines[2]
        .matches
        .iter()
        .map(|r| &lines[2].text[r.clone()])
        .collect();
    assert!(matched.contains(&"CAFÉ"));
    assert!(matched.contains(&"project budget"));
    assert!(matched.contains(&"İ"));
    std::fs::write(&path, format!("{}é", "a".repeat(TEXT_LIMIT - 1)))?;
    assert!(matches!(
        load(&path, "missing")?,
        Content::Text {
            truncated: true,
            ..
        }
    ));
    std::fs::write(&path, "line\n".repeat(25_000))?;
    let Content::Text {
        lines, truncated, ..
    } = load(&path, "line")?
    else {
        panic!()
    };
    assert!(truncated);
    assert_eq!(lines.len(), 20_000);
    Ok(())
}

#[test]
fn image_preview_is_bounded_and_bad_files_fail_cleanly() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("image.png");
    image::RgbImage::from_pixel(2000, 1000, image::Rgb([10, 40, 80])).save(&path)?;
    let Content::Image {
        width,
        height,
        rgba,
        original,
    } = load(&path, "")?
    else {
        panic!("expected image")
    };
    assert_eq!(original, (2000, 1000));
    assert_eq!((width, height), (1600, 800));
    assert_eq!(rgba.len(), width * height * 4);
    std::fs::write(&path, b"not an image")?;
    assert!(load(&path, "").is_err());
    let binary = temp.path().join("binary.bin");
    std::fs::write(&binary, [0, 1, 2, 3])?;
    assert!(load(&binary, "").is_err());
    assert!(load(&temp.path().join("missing"), "").is_err());
    assert!(load(temp.path(), "").is_err());
    Ok(())
}
