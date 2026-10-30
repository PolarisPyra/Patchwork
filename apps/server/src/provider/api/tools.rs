//! The tools Patchwork's own agent gives an API model: reading, searching, and changing the
//! project's files, running shell commands, and fetching web pages.

mod describe;
mod files;
mod shell;
mod web;

pub use describe::{definitions, display_name, summary, shown, approval, writes};
use files::{READ_LINES, read_file, write_file, edit_file, list_dir, glob, grep};
use shell::{BASH_TIMEOUT, MAX_BASH_TIMEOUT, bash};
use web::fetch_url;

use std::path::Path;

use serde_json::Value;

use crate::system::fs::resolve;
use crate::computer;

/// Longest tool result sent back to the model; the middle of a longer one is cut.
const MAX_RESULT: usize = 40_000;

/// An integer argument.
fn number(args: &Value, key: &str) -> Result<i32, String> {
    args[key]
        .as_i64()
        .map(|n| n.clamp(i32::MIN as i64, i32::MAX as i64) as i32)
        .ok_or_else(|| format!("{key} is required"))
}

/// Takes a screenshot and returns its path, for the model to read as an image.
fn computer_screenshot() -> Result<String, String> {
    let dir = crate::system::dirs().cache.join("screenshots");
    let path = crate::computer::screenshot_to_file(&dir)?;
    Ok(format!(
        "Screenshot written to {}. Read it with read_file to see the screen.",
        path.display()
    ))
}

/// Runs a file, search, shell, or web tool. Ok carries the result for the model; Err a failure
/// the model is told about.
pub fn run(name: &str, args: &Value, root: &Path) -> Result<String, String> {
    let path = || {
        args["path"]
            .as_str()
            .filter(|p| !p.trim().is_empty())
            .map(|p| resolve(root, p))
            .ok_or_else(|| "path is required".to_string())
    };
    let text = |key: &str| {
        args[key]
            .as_str()
            .map(String::from)
            .ok_or_else(|| format!("{key} is required"))
    };
    let result = match name {
        "read_file" => read_file(
            &path()?,
            args["offset"].as_u64().unwrap_or(1).max(1) as usize,
            args["limit"].as_u64().map_or(READ_LINES, |l| l as usize),
        ),
        "write_file" => write_file(&path()?, &text("content")?, root),
        "edit_file" => edit_file(
            &path()?,
            &text("old_string")?,
            &text("new_string")?,
            args["replace_all"] == true,
            root,
        ),
        "list_dir" => list_dir(&path().unwrap_or_else(|_| root.to_path_buf()), root),
        "glob" => glob(
            &text("pattern")?,
            &path().unwrap_or_else(|_| root.to_path_buf()),
            root,
        ),
        "grep" => grep(
            &text("pattern")?,
            &path().unwrap_or_else(|_| root.to_path_buf()),
            args["glob"].as_str(),
            args["case_insensitive"] == true,
            root,
        ),
        "bash" => bash(
            &text("command")?,
            args["timeout"]
                .as_u64()
                .unwrap_or(BASH_TIMEOUT)
                .clamp(1, MAX_BASH_TIMEOUT),
            root,
        ),
        "fetch_url" => fetch_url(&text("url")?),
        "computer_screenshot" => computer_screenshot(),
        "computer_move" => computer::perform(&computer::Action::Move {
            x: number(args, "x")?,
            y: number(args, "y")?,
        })
        .map(|()| "Moved the pointer".to_string()),
        "computer_click" => {
            let action = computer::Action::Click {
                x: number(args, "x")?,
                y: number(args, "y")?,
                button: args["button"].as_str().unwrap_or("left").to_string(),
                double: args["double"] == true,
            };
            computer::perform(&action).map(|()| computer::describe(&action))
        }
        "computer_type" => {
            let action = computer::Action::Type {
                text: text("text")?,
            };
            computer::perform(&action).map(|()| computer::describe(&action))
        }
        "computer_key" => {
            let action = computer::Action::Key {
                key: text("key")?,
                modifiers: args["modifiers"]
                    .as_array()
                    .map(|list| {
                        list.iter()
                            .filter_map(|m| m.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            computer::perform(&action).map(|()| computer::describe(&action))
        }
        "computer_scroll" => {
            let action = computer::Action::Scroll {
                amount: number(args, "amount")?,
                horizontal: args["horizontal"] == true,
            };
            computer::perform(&action).map(|()| computer::describe(&action))
        }
        other => Err(format!("There is no tool named {other}")),
    }?;
    Ok(cap(result))
}

/// Cuts a long result to its start and end.
pub fn cap(text: String) -> String {
    if text.len() <= MAX_RESULT {
        return text;
    }
    let mut head = MAX_RESULT * 3 / 4;
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len() - MAX_RESULT / 4;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    let cut = text[head..tail].lines().count();
    format!(
        "{}\n\n[… {cut} lines cut …]\n\n{}",
        &text[..head],
        &text[tail..]
    )
}
