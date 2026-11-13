//! Running the model's tool calls: Patchwork's own tools, MCP tools, the tool hooks, and asking the user.

// Rust guideline compliant 2026-10-09

use crate::Error;
use std::fmt::Write as _;
use serde_json::{Value, json};
use crate::hooks::HookEvent;
use crate::permissions::Target;
use crate::permissions::PermissionKind;
use crate::threads::AccessMode;
use super::request::Call;
use super::{Agent, ImageIn, emit};

const DENIED: &str = "The user denied this. Do not retry it; ask what they would like instead, or carry on another way.";
const NO_RESULT: &str = "The turn ended before the tool finished";
const PLAN_SHOWN: &str = "The plan is shown to the user. Call update_plan again with the whole list each time a step starts or finishes.";

impl Agent {
    /// Runs one tool call and returns its result for the model, and whether the turn ends here.
    pub(super) fn call(&mut self, call: &Call) -> (String, bool) {
        let args: Value = match serde_json::from_str(if call.arguments.trim().is_empty() {
            "{}"
        } else {
            &call.arguments
        }) {
            Ok(args) => args,
            Err(e) => {
                let message = if self.cut_off {
                    format!(
                        "Your reply reached the output limit, so these arguments were cut off ({e}). Make the call again with less in it, such as a shorter file written first and the rest added with edit_file."
                    )
                } else {
                    format!("The arguments were not valid JSON ({e}); call the tool again")
                };
                emit(&json!({"type": "api.tool", "id": call.id, "name": call.name, "summary": ""}));
                emit(
                    &json!({"type": "api.tool_done", "id": call.id, "error": true, "output": message}),
                );
                return (message, false);
            }
        };
        let summary = crate::harness::tools::summary(&call.name, &args, &self.root);
        emit(&json!({"type": "api.tool", "id": call.id, "name": call.name, "summary": summary}));
        if let Some(reason) = self.run_hooks(HookEvent::PreToolUse, &call.name, &args, None) {
            emit(&json!({"type": "api.tool_done", "id": call.id, "error": true, "output": reason}));
            return (reason, false);
        }
        let mcp = self
            .request
            .mcp_tools
            .iter()
            .position(|t| t.name == call.name);
        let (result, error, stop) = if let Some(index) = mcp {
            let result = self.call_mcp(index, &call.id, &args);
            let error = result.is_err();
            (result, error, false)
        } else {
            match call.name.as_str() {
                "update_plan" => {
                    emit(&json!({"type": "api.todos", "steps": args["steps"]}));
                    (Ok(PLAN_SHOWN.to_string()), false, false)
                }
                "propose_plan" => {
                    let plan = args["plan"].as_str().unwrap_or_default();
                    emit(&json!({"type": "api.plan", "text": plan}));
                    (
                        Ok("The plan was shown to the user, who will reply.".to_string()),
                        false,
                        true,
                    )
                }
                "ask_user" => match self.ask(&call.id, &args) {
                    Ok(answer) => (Ok(answer), false, false),
                    Err(e) => (Err(e), true, false),
                },
                crate::secrets::TOOL => {
                    let result = self.request_secret(&call.id, &args);
                    let error = result.is_err();
                    (result, error, false)
                }
                // The commands Patchwork holds for this agent, which reading or stopping asks no one.
                "job_output" | "job_kill" | "job_list" => {
                    let result = self.call_own(&call.id, &call.name, &args, NO_RESULT);
                    let error = result.is_err();
                    (result, error, false)
                }
                name => {
                    // A write that would be refused anyway is not put to the user first.
                    let stale = self.stale(name, &args);
                    let approval =
                        crate::harness::tools::approval(name, &args, self.request.mode, &self.root)
                            .filter(|_| stale.is_none() && !self.reads_saved_output(name, &args));
                    let allowed = match approval {
                        Some((title, detail, target, asks)) => {
                            self.approve(&call.id, &title, &detail, target, asks)
                        }
                        None => true,
                    };
                    if let Some(stale) = stale {
                        (Err(stale.into()), true, false)
                    } else if allowed
                        && name == "read_file"
                        && let Some(image) = self.read_image(&args)
                    {
                        let error = image.is_err();
                        (image, error, false)
                    } else if allowed && name == "bash" {
                        // Patchwork runs it, so it can carry on as a job past its time limit.
                        let result = self.call_own(&call.id, name, &args, NO_RESULT);
                        let error = result.is_err();
                        (result, error, false)
                    } else if allowed {
                        let result = self
                            .prefetched
                            .remove(&call.id)
                            .unwrap_or_else(|| crate::harness::tools::run(name, &args, &self.root));
                        if result.is_ok() {
                            self.saw(name, &args);
                        }
                        let error = result.is_err();
                        (result, error, false)
                    } else {
                        (Err(DENIED.into()), true, false)
                    }
                }
            }
        };
        let text = match result {
            Ok(text) => self.fit(&call.id, text),
            Err(e) => self.fit(&call.id, e.to_string()),
        };
        // The call has run, so a refusal here only tells the model.
        let mut text = match self.run_hooks(HookEvent::PostToolUse, &call.name, &args, Some(&text))
        {
            Some(reason) => format!("{text}\n\n{reason}"),
            None => text,
        };
        if let Some(nudge) = self.repeated(&call.name, &args) {
            let _ = write!(text, "\n\n{nudge}");
        }
        let mut done =
            json!({"type": "api.tool_done", "id": call.id, "error": error, "output": text});
        if !error
            && crate::harness::tools::writes(&call.name)
            && let Some(path) = args["path"].as_str()
        {
            done["path"] = json!(crate::harness::tools::shown(
                &crate::system::fs::resolve(&self.root, path),
                &self.root
            ));
        }
        emit(&done);
        (text, stop)
    }

    /// Runs the hooks for `event` that match the tool, with the call as Claude Code gives
    /// it to its hooks, and returns the first one's reason for refusing it.
    pub(super) fn run_hooks(
        &self,
        event: HookEvent,
        tool: &str,
        args: &Value,
        response: Option<&str>,
    ) -> Option<String> {
        let hooks = self
            .request
            .hooks
            .iter()
            .filter(|hook| hook.event == event && hook.matches(tool));
        let mut input = json!({
            "session_id": self.session_id,
            "cwd": self.request.root,
            "tool_name": tool,
            "tool_input": args,
        });
        if let Some(response) = response {
            input["tool_response"] = Value::from(response);
        }
        hooks.into_iter().find_map(|hook| {
            input["hook_event_name"] = Value::from(hook.event.cli_name());
            crate::hooks::run_tool_hook(hook, &input, &self.request.root)
        })
    }

    /// Runs the reads the step starts with all at once, since they change nothing, so a model
    /// that reads several files waits for the slowest rather than for each in turn. Only reads
    /// that run without asking anyone are run early; the rest wait for their turn in `call`.
    pub(super) fn prefetch(&mut self, calls: &[Call]) {
        let reads: Vec<(&Call, Value)> = calls
            .iter()
            .map_while(|call| {
                let args: Value = serde_json::from_str(&call.arguments).ok()?;
                self.reads_alone(&call.name, &args).then_some((call, args))
            })
            .collect();
        if reads.len() < 2 {
            return;
        }
        let root = &self.root;
        let results: Vec<(String, Result<String, Error>)> = std::thread::scope(|s| {
            let running: Vec<_> = reads
                .iter()
                .map(|(call, args)| {
                    s.spawn(move || crate::harness::tools::run(&call.name, args, root))
                })
                .collect();
            running
                .into_iter()
                .zip(&reads)
                .filter_map(|(run, (call, _))| Some((call.id.clone(), run.join().ok()?)))
                .collect()
        });
        self.prefetched.extend(results);
    }

    /// Whether a call only reads the project, with no hook, approval, or permission rule to ask.
    fn reads_alone(&self, name: &str, args: &Value) -> bool {
        matches!(name, "read_file" | "list_dir" | "glob" | "grep")
            && !self.request.mcp_tools.iter().any(|t| t.name == name)
            && !self.request.hooks.iter().any(|h| h.matches(name))
            && crate::harness::tools::approval(name, args, self.request.mode, &self.root).is_none()
            && !(name == "read_file"
                && args["path"]
                    .as_str()
                    .is_some_and(|p| crate::threads::image_media_type(p).is_some()))
    }

    /// Why a write would undo a change made to the file since the model last read or wrote it,
    /// as by the user meanwhile.
    fn stale(&self, name: &str, args: &Value) -> Option<String> {
        if name != "write_file" {
            return None;
        }
        let path = crate::system::fs::resolve(&self.root, args["path"].as_str()?);
        let seen = self.seen.get(&path)?;
        (version(&path)? != *seen).then(|| {
            format!(
                "{} changed since you last read it, and writing it would undo that change. Read it again, then make your change.",
                crate::harness::tools::shown(&path, &self.root)
            )
        })
    }

    /// Notes the file a read or write left, for `stale`.
    fn saw(&mut self, name: &str, args: &Value) {
        if matches!(name, "read_file" | "write_file" | "edit_file")
            && let Some(path) = args["path"].as_str()
        {
            let path = crate::system::fs::resolve(&self.root, path);
            if let Some(version) = version(&path) {
                self.seen.insert(path, version);
            }
        }
    }

    /// A reminder for a model making the same call again and again, as one stuck in a loop does:
    /// at the 3rd, 5th, and 8th time in a row. Anything in between, such as an edit before the
    /// same build, starts the count again. Reading a job is left out, since its answer changes
    /// as the job runs.
    fn repeated(&mut self, name: &str, args: &Value) -> Option<String> {
        if matches!(name, "job_output" | "job_list") {
            return None;
        }
        let key = format!("{name} {args}");
        if self.last_call.0 == key {
            self.last_call.1 += 1;
        } else {
            self.last_call = (key, 1);
        }
        let times = self.last_call.1;
        matches!(times, 3 | 5 | 8).then(|| {
            format!(
                "(You have now made this exact call {times} times in a row, and the same call gives the same result. Try a different approach, or tell the user what is in the way.)"
            )
        })
    }

    /// A result the model can take whole, or its start and end, with the whole of it saved to a
    /// file in the session's folder for the model to read on.
    pub(super) fn fit(&self, id: &str, text: String) -> String {
        if text.len() <= crate::harness::tools::MAX_RESULT {
            return text;
        }
        let name: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            .collect();
        let name = if name.is_empty() {
            uuid::Uuid::new_v4().simple().to_string()
        } else {
            name
        };
        let path = self.saved_output_dir().join(format!("{name}.txt"));
        let saved = crate::system::fs::write_private(&path, &text).is_ok();
        crate::harness::tools::cap(text, saved.then_some(path.as_path()))
    }

    /// Whether a call only reads output `fit` saved, which needs no approval though it is outside
    /// the project.
    fn reads_saved_output(&self, name: &str, args: &Value) -> bool {
        matches!(name, "read_file" | "grep")
            && args["path"].as_str().is_some_and(|p| {
                let path = crate::system::fs::resolve(&self.root, p);
                // A path that climbs out with `..` does not count.
                !p.contains("..") && path.starts_with(self.saved_output_dir())
            })
    }

    /// Opens the image `read_file` was pointed at for the model to see, or None when the path is
    /// not an image.
    pub(super) fn read_image(&mut self, args: &Value) -> Option<Result<String, Error>> {
        let path = crate::system::fs::resolve(&self.root, args["path"].as_str()?);
        let path = path.to_str()?;
        crate::threads::image_media_type(path)?;
        let image = crate::threads::Attachment::from_path(path)
            .and_then(|a| crate::provider::load_images(&[a]))
            .map(|mut images| images.remove(0));
        Some(image.map(|image| {
            self.read_images.push(ImageIn {
                media_type: image.media_type.into(),
                data: image.data,
            });
            format!("{path} is an image; it follows the tool results.")
        }))
    }

    /// Has Patchwork call an MCP tool, once the user approves where the access mode asks and the
    /// permission rules allow it.
    pub(super) fn call_mcp(
        &mut self,
        index: usize,
        id: &str,
        args: &Value,
    ) -> Result<String, Error> {
        let tool = &self.request.mcp_tools[index];
        let asks = match self.request.mode {
            AccessMode::FullAccess => false,
            AccessMode::Supervised => true,
            _ => !tool.read_only,
        };
        let (server, name) = (tool.server.clone(), tool.tool.clone());
        if !self.approve(
            id,
            &format!("Use {name} from {server}"),
            &args.to_string(),
            Target::new(PermissionKind::Mcp, [format!("{server}/{name}")]),
            asks,
        ) {
            return Err(DENIED.into());
        }
        emit(
            &json!({"type": "api.mcp_call", "id": id, "server": server, "tool": name, "arguments": args}),
        );
        let reply = self
            .wait(id)
            .ok_or("The turn ended before the tool finished")?;
        let output = reply["output"].as_str().unwrap_or_default().to_string();
        if reply["error"] == true {
            Err(output.into())
        } else {
            Ok(output)
        }
    }

    /// Has Patchwork ask the user for a secret, which is theirs to answer, so it is not approved
    /// first, and waits for where it was saved.
    fn request_secret(&mut self, id: &str, args: &Value) -> Result<String, Error> {
        self.call_own(
            id,
            crate::secrets::TOOL,
            args,
            "The turn ended before the user answered",
        )
    }

    /// Has Patchwork run one of its own tools, and waits for the result; `ended` is the error
    /// when the turn ends first.
    fn call_own(
        &mut self,
        id: &str,
        tool: &str,
        args: &Value,
        ended: &str,
    ) -> Result<String, Error> {
        emit(&json!({
            "type": "api.mcp_call", "id": id, "server": crate::provider::mcp::OWN_SERVER,
            "tool": tool, "arguments": args,
        }));
        let reply = self.wait(id).ok_or(ended)?;
        let output = reply["output"].as_str().unwrap_or_default().to_string();
        if reply["error"] == true {
            Err(output.into())
        } else {
            Ok(output)
        }
    }

    /// Asks Patchwork for an approval, or with `asks` false for the permission rules' answer to a
    /// call the access mode runs without asking, and waits for the answer.
    pub(super) fn approve(
        &mut self,
        id: &str,
        title: &str,
        detail: &str,
        target: Target,
        asks: bool,
    ) -> bool {
        let Target { kind, resources } = target;
        emit(&json!({
            "type": if asks { "api.approval" } else { "api.check" }, "id": id, "title": title, "detail": detail,
            "kind": kind, "resources": resources,
        }));
        self.wait(id).is_some_and(|reply| reply["allow"] == true)
    }

    pub(super) fn ask(&mut self, id: &str, args: &Value) -> Result<String, Error> {
        let questions: Vec<Value> = args["questions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(i, q)| {
                json!({
                    "id": format!("q{i}"),
                    "header": q["header"].as_str().unwrap_or_default(),
                    "question": q["question"].as_str().unwrap_or_default(),
                    "options": q["options"],
                    "multiSelect": q["multi_select"] == true,
                })
            })
            .collect();
        if questions.is_empty() {
            return Err("Give at least one question".into());
        }
        emit(&json!({"type": "api.question", "id": id, "questions": questions}));
        let reply = self
            .wait(id)
            .ok_or("The turn ended before the user answered")?;
        if reply["allow"] != true {
            return Err(
                "The user dismissed the questions. Carry on with your best judgment.".into(),
            );
        }
        let lines: Vec<String> = questions
            .iter()
            .map(|q| {
                let qid = q["id"].as_str().unwrap_or_default();
                let answer: Vec<&str> = reply["answers"][qid]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let answer = if answer.is_empty() {
                    "(no answer)".into()
                } else {
                    answer.join(", ")
                };
                format!("{}: {answer}", q["question"].as_str().unwrap_or_default())
            })
            .collect();
        Ok(format!("The user answered:\n{}", lines.join("\n")))
    }

    /// Waits for the reply to the request `id`, keeping steering messages that come meanwhile.
    pub(super) fn wait(&mut self, id: &str) -> Option<Value> {
        while let Ok(v) = self.input.recv() {
            match v["type"].as_str() {
                Some("reply") if v["id"] == id => return Some(v),
                Some("steer" | "compact") => self.steers.push(v),
                Some("compactWindow") => {
                    self.request.compact_window = v["window"].as_u64().unwrap_or(0);
                }
                _ => {}
            }
        }
        self.closed = true;
        None
    }
}

/// A file's modified time and length, which change whenever it is written.
fn version(path: &std::path::Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}
