//! The account's audit log: what happened to it, on which device, and from which address.

use sqlx::PgExecutor;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Event {
    Registered,
    SignedIn,
    SignedOut,
    SignedOutEverywhere,
    EmailChanged,
    PasswordChanged,
    PasswordReset,
    DeviceRevoked,
    AccountDeleted,
    TrialStarted,
}

impl Event {
    fn name(self) -> &'static str {
        match self {
            Event::Registered => "registered",
            Event::SignedIn => "signed_in",
            Event::SignedOut => "signed_out",
            Event::SignedOutEverywhere => "signed_out_everywhere",
            Event::EmailChanged => "email_changed",
            Event::PasswordChanged => "password_changed",
            Event::PasswordReset => "password_reset",
            Event::DeviceRevoked => "device_revoked",
            Event::AccountDeleted => "account_deleted",
            Event::TrialStarted => "trial_started",
        }
    }
}

pub(crate) async fn record<'e>(
    db: impl PgExecutor<'e>,
    event: Event,
    user: Option<Uuid>,
    device: Option<Uuid>,
    ip: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query("insert into events (user_id, device_id, type, ip) values ($1, $2, $3, $4::inet)")
        .bind(user)
        .bind(device)
        .bind(event.name())
        .bind(ip)
        .execute(db)
        .await?;
    Ok(())
}
