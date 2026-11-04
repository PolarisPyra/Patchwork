//! Sending the service's emails: over SMTP in production, or to the log on a developer's machine.

use lettre::message::Mailbox;
use lettre::message::header::ContentType;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::config::MailConfig;

#[derive(Clone)]
pub(crate) enum Mailer {
    Smtp {
        transport: Box<AsyncSmtpTransport<Tokio1Executor>>,
        from: Mailbox,
    },
    Log,
}

impl Mailer {
    pub(crate) fn new(config: &MailConfig) -> Result<Self, String> {
        match config {
            MailConfig::Log => {
                log::warn!("emails go to this log, not to anyone's inbox");
                Ok(Mailer::Log)
            }
            MailConfig::Smtp { url, from } => Ok(Mailer::Smtp {
                transport: Box::new(
                    AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
                        .map_err(|e| format!("PATCHWORK_API_SMTP_URL: {e}"))?
                        .build(),
                ),
                from: from
                    .parse()
                    .map_err(|e| format!("PATCHWORK_API_MAIL_FROM: {e}"))?,
            }),
        }
    }

    /// Sends a plain-text email to `to`, with a Message-ID in the sender's domain.
    pub(crate) async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), String> {
        let (transport, from) = match self {
            Mailer::Log => {
                log::info!("email to {to}: {subject}\n{body}");
                return Ok(());
            }
            Mailer::Smtp { transport, from } => (transport, from),
        };
        let to: Mailbox = to.parse().map_err(|e| format!("{to}: {e}"))?;
        let id = format!("<{}@{}>", uuid::Uuid::new_v4(), from.email.domain());
        let message = Message::builder()
            .message_id(Some(id))
            .from(from.clone())
            .to(to.clone())
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(body.to_owned())
            .map_err(|e| e.to_string())?;
        transport
            .send(message)
            .await
            .map(|_| ())
            .map_err(|e| format!("couldn't send email to {to}: {e}"))
    }
}
