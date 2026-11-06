//! Bounded ADF-to-text conversion; raw ADF is retained separately in remote_items.
use anyhow::{ensure, Result};
use serde_json::Value;
#[derive(Debug)]
pub struct Extracted {
    pub text: String,
    pub warnings: Vec<String>,
}
pub fn extract(value: &Value) -> Result<Extracted> {
    let mut out = Extracted {
        text: String::new(),
        warnings: vec![],
    };
    if value.is_null() {
        return Ok(out);
    }
    if let Some(text) = value.as_str() {
        ensure!(text.len() <= 2 * 1024 * 1024, "Description exceeds 2 MiB");
        out.text = text.into();
        out.warnings
            .push("Description is plain text; structured ADF was unavailable".into());
        return Ok(out);
    }
    ensure!(value["type"] == "doc", "Description is not an ADF document");
    let mut nodes = 0;
    walk(value, &mut out, 0, &mut nodes)?;
    out.text = out.text.trim().to_owned();
    out.warnings.sort();
    out.warnings.dedup();
    Ok(out)
}
fn walk(v: &Value, out: &mut Extracted, depth: usize, nodes: &mut usize) -> Result<()> {
    *nodes += 1;
    ensure!(
        depth <= 64 && *nodes <= 100_000,
        "ADF structure exceeds safe conversion limits"
    );
    let kind = v["type"].as_str().unwrap_or("unknown");
    match kind {
        "text" => {
            if !v["text"].is_string() {
                out.warnings
                    .push("Incomplete ADF text node: missing text".into());
            }
            out.text.push_str(v["text"].as_str().unwrap_or(""));
            if let Some(marks) = v["marks"].as_array() {
                for mark in marks {
                    if mark["type"] == "link" {
                        if let Some(href) = mark["attrs"]["href"].as_str() {
                            out.text.push_str(&format!(" ({href})"));
                        }
                    }
                }
            }
        }
        "hardBreak" => out.text.push('\n'),
        "heading" => {
            out.text.push('\n');
            out.text.push_str(
                &"#".repeat(v["attrs"]["level"].as_u64().unwrap_or(1).clamp(1, 6) as usize),
            );
            out.text.push(' ');
        }
        "listItem" | "taskItem" => out.text.push_str("\n- "),
        "tableCell" | "tableHeader" => out.text.push_str(" | "),
        "codeBlock" => out.text.push_str("\n```\n"),
        "blockquote" => out.text.push_str("\n> "),
        "rule" => out.text.push_str("\n---\n"),
        "mention" => out
            .text
            .push_str(v["attrs"]["text"].as_str().unwrap_or("[mention]")),
        "emoji" => out.text.push_str(
            v["attrs"]["text"]
                .as_str()
                .or(v["attrs"]["shortName"].as_str())
                .unwrap_or("[emoji]"),
        ),
        "inlineCard" | "blockCard" => out
            .text
            .push_str(v["attrs"]["url"].as_str().unwrap_or("[link]")),
        "doc" | "paragraph" | "bulletList" | "orderedList" | "table" | "tableRow" | "panel"
        | "taskList" => {}
        other => {
            out.warnings.push(format!("Unsupported ADF node: {other}"));
            out.text
                .push_str(&format!("\n[Unsupported content: {other}]\n"));
        }
    }
    if let Some(children) = v["content"].as_array() {
        for child in children {
            walk(child, out, depth + 1, nodes)?;
        }
    }
    match kind {
        "paragraph" | "heading" | "tableRow" | "blockquote" | "listItem" | "panel" => {
            out.text.push('\n')
        }
        "table" | "bulletList" | "orderedList" => out.text.push('\n'),
        "codeBlock" => out.text.push_str("\n```\n"),
        _ => {}
    }
    ensure!(
        out.text.len() <= 2 * 1024 * 1024,
        "Converted description exceeds 2 MiB; content was not silently truncated"
    );
    Ok(())
}
