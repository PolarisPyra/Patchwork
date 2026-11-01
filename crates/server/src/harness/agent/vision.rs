//! Reading images for a model that cannot: the vision model describes them, and the model works
//! from the description.

use serde_json::{Value, json};
use super::{Agent, ImageIn, Vision};

/// What the vision model is asked for. The user's message follows, so it knows what matters.
const DESCRIBE: &str = "Another model, which cannot see images, will answer the user's message below from your description of the attached images. Describe each image fully: transcribe every piece of visible text exactly, including code, settings, field names and values, labels and error messages, and describe the layout and anything else that could matter to the answer. Do not answer the message yourself.";

impl Agent {
    /// `text` with the vision model's description of `images` after it, or a note for the
    /// model to pass on when the vision model could not describe them.
    pub(super) fn described(&self, vision: &Vision, text: &str, images: &[ImageIn]) -> String {
        let what = if images.len() == 1 {
            "the image"
        } else {
            "the images"
        };
        let model = &vision.model;
        match self.describe(vision, text, images) {
            Ok(description) => format!(
                "{text}\n\n(You cannot see images, so the vision model {model} describes {what} here:)\n\n{description}"
            ),
            Err(e) => format!(
                "{text}\n\n(You cannot see images, and the vision model {model} could not describe {what}: {e}. Tell the user.)"
            ),
        }
    }

    /// Asks the vision model to describe `images`, with the user's message beside them.
    fn describe(&self, vision: &Vision, text: &str, images: &[ImageIn]) -> Result<String, String> {
        let mut content = vec![
            json!({"type": "text", "text": format!("{DESCRIBE}\n\nThe user's message:\n{text}")}),
        ];
        content.extend(images.iter().map(|i| {
            json!({"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", i.media_type, i.data)}})
        }));
        let body = json!({
            "model": vision.model,
            "messages": [{"role": "user", "content": content}],
            "stream": false,
        });
        let url = format!("{}/chat/completions", vision.base_url.trim_end_matches('/'));
        let mut request = self
            .http
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(key) = &vision.api_key {
            request = request.header("Authorization", &format!("Bearer {key}"));
        }
        for (name, value) in &vision.headers {
            request = request.header(name, value);
        }
        let mut response = request
            .send(body.to_string())
            .map_err(|e| format!("could not reach {}: {e}", vision.provider))?;
        let status = response.status().as_u16();
        let raw = response.body_mut().read_to_string().unwrap_or_default();
        let parsed: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        if !response.status().is_success() {
            return Err(crate::harness::error_text(status, &parsed, &raw));
        }
        let reply = parsed["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        // A reasoning model served without a reasoning parser thinks inside its reply.
        let reply = reply
            .rsplit_once("</think>")
            .map_or(reply, |(_, after)| after)
            .trim();
        if reply.is_empty() {
            return Err(format!("{} sent back no description", vision.provider));
        }
        Ok(reply.to_string())
    }
}
