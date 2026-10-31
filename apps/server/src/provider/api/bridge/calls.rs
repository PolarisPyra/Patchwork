//! Running the model's tool calls: Patchwork's own tools, MCP tools, the plugins' tool hooks, and asking the user.

use serde_json::{Value, json};
use crate::plugins::HookEvent;
use crate::permissions::Target;
use crate::permissions::PermissionKind;
use crate::threads::AccessMode;
use super::{Agent, Call, ImageIn, emit};

const DENIED: &str = "The user denied this. Do not retry it; ask what they would like instead, or carry on another way.";
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
                let message =
                    format!("The arguments were not valid JSON ({e}); call the tool again");
                emit(&json!({"type": "api.tool", "id": call.id, "name": call.name, "summary": ""}));
                emit(
                    &json!({"type": "api.tool_done", "id": call.id, "error": true, "output": message}),
                );
                return (message, false);
            }
        };
        let summary = crate::provider::api::tools::summary(&call.name, &args, &self.root);
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
                name => {
                    let allowed = match crate::provider::api::tools::approval(
                        name,
                        &args,
                        self.request.mode,
                        &self.root,
                    ) {
                        Some((title, detail, target, asks)) => {
                            self.approve(&call.id, &title, &detail, target, asks)
                        }
                        None => true,
                    };
                    if allowed
                        && name == "read_file"
                        && let Some(image) = self.read_image(&args)
                    {
                        let error = image.is_err();
                        (image, error, false)
                    } else if allowed {
                        let result = crate::provider::api::tools::run(name, &args, &self.root);
                        let error = result.is_err();
                        (result, error, false)
                    } else {
                        (Err(DENIED.into()), true, false)
                    }
                }
            }
        };
        let text = match &result {
            Ok(text) | Err(text) => text.clone(),
        };
        // The call has run, so a refusal here only tells the model.
        let text = match self.run_hooks(HookEvent::PostToolUse, &call.name, &args, Some(&text)) {
            Some(reason) => format!("{text}\n\n{reason}"),
            None => text,
        };
        let mut done =
            json!({"type": "api.tool_done", "id": call.id, "error": error, "output": text});
        if !error
            && crate::provider::api::tools::writes(&call.name)
            && let Some(path) = args["path"].as_str()
        {
            done["path"] = json!(crate::provider::api::tools::shown(
                &crate::system::fs::resolve(&self.root, path),
                &self.root
            ));
        }
        emit(&done);
        (text, stop)
    }

    /// Runs the plugins' hooks for `event` that match the tool, with the call as Claude Code gives
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
            crate::plugins::run_tool_hook(hook, &input, &self.request.root)
        })
    }

    /// Opens the image `read_file` was pointed at for the model to see, or None when the path is
    /// not an image.
    pub(super) fn read_image(&mut self, args: &Value) -> Option<Result<String, String>> {
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
    ) -> Result<String, String> {
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
        let output = crate::provider::api::tools::cap(
            reply["output"].as_str().unwrap_or_default().to_string(),
        );
        if reply["error"] == true {
            Err(output)
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

    pub(super) fn ask(&mut self, id: &str, args: &Value) -> Result<String, String> {
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
                Some("steer") => self.steers.push(v),
                _ => {}
            }
        }
        self.closed = true;
        None
    }
}
