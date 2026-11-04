//! One-time codes sent by email: six digits, good for ten minutes and five tries. Only an HMAC of
//! each is kept, made with a key outside the database, so a copy of the database checks none.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

use crate::Api;
use crate::http::{ApiError, Res};

const ATTEMPTS: i32 = 5;
/// Emails one address can be sent in an hour, and how soon after one the next can be asked for.
const PER_ADDRESS_PER_HOUR: i64 = 5;
const PER_ADDRESS_PER_30_SECONDS: i64 = 1;
/// Emails one caller can ask for in an hour, across every address.
const PER_IP_PER_HOUR: i64 = 30;
const TOO_MANY: &str = "Too many codes were asked for. Wait a few minutes and try again";

/// What a code lets its holder do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "code_purpose", rename_all = "snake_case")]
pub(crate) enum Purpose {
    Register,
    TwoFactor,
    ResetPassword,
    ChangeEmail,
}

impl Purpose {
    /// The email's subject and text.
    fn wording(self, code: &str) -> (&'static str, String) {
        match self {
            Purpose::Register => (
                "Confirm your email for Patchwork",
                format!(
                    "Enter {code} in Patchwork to finish making your account.\n\nIt works for 10 \
                     minutes. If you didn't ask for this, you can ignore this email."
                ),
            ),
            Purpose::TwoFactor => (
                "Your Patchwork sign-in code",
                format!(
                    "Your code is {code}.\n\nIt works for 10 minutes. Someone signed in with your \
                     password: if it wasn't you, change your password in Patchwork."
                ),
            ),
            Purpose::ResetPassword => (
                "Reset your Patchwork password",
                format!(
                    "Enter {code} in Patchwork to choose a new password.\n\nIt works for 10 \
                     minutes. If you didn't ask for this, you can ignore this email."
                ),
            ),
            Purpose::ChangeEmail => (
                "Confirm your new Patchwork email",
                format!(
                    "Enter {code} in Patchwork to make this your account's email.\n\nIt works for 10 \
                     minutes. If you didn't ask for this, you can ignore this email."
                ),
            ),
        }
    }
}

/// Which code [`redeem`] looks for: the newest one sent to an address or made for an account, or
/// one by its id.
pub(crate) enum By<'a> {
    Email(&'a str),
    User(Uuid),
    Id(Uuid),
}

/// A code that matched, and who it was made for.
pub(crate) struct Redeemed {
    pub email: String,
    pub user_id: Option<Uuid>,
}

/// Makes a code for `email`, keeps its HMAC, and sends it. Returns the code's id.
pub(crate) async fn issue(
    api: &Api,
    purpose: Purpose,
    email: &str,
    user: Option<Uuid>,
    ip: Option<&str>,
) -> Res<Uuid> {
    let (id, code) = create(api, purpose, email, user, ip, false).await?;
    let (subject, body) = purpose.wording(&code);
    send(api, email, subject, &body).await?;
    Ok(id)
}

/// Stands in for [`issue`] when a code would be wrong to send but the answer must look the same:
/// it counts against the same limits, and sends `instead` (a subject and text) if given.
pub(crate) async fn decoy(
    api: &Api,
    purpose: Purpose,
    email: &str,
    ip: Option<&str>,
    instead: Option<(&str, &str)>,
) -> Res<()> {
    create(api, purpose, email, None, ip, true).await?;
    match instead {
        Some((subject, body)) => send(api, email, subject, body).await,
        None => Ok(()),
    }
}

/// Checks the limits, then keeps a new code, already used up when `used` is set so it only counts
/// against them.
async fn create(
    api: &Api,
    purpose: Purpose,
    email: &str,
    user: Option<Uuid>,
    ip: Option<&str>,
    used: bool,
) -> Res<(Uuid, String)> {
    let (hour, recent): (i64, i64) = sqlx::query_as(
        "select count(*), count(*) filter (where created_at > now() - interval '30 seconds')
         from email_codes where email = $1::citext and created_at > now() - interval '1 hour'",
    )
    .bind(email)
    .fetch_one(&api.db)
    .await?;
    if hour >= PER_ADDRESS_PER_HOUR || recent >= PER_ADDRESS_PER_30_SECONDS {
        return Err(ApiError::too_many(TOO_MANY));
    }
    if let Some(ip) = ip {
        let (count,): (i64,) = sqlx::query_as(
            "select count(*) from email_codes
             where ip = $1::inet and created_at > now() - interval '1 hour'",
        )
        .bind(ip)
        .fetch_one(&api.db)
        .await?;
        if count >= PER_IP_PER_HOUR {
            return Err(ApiError::too_many(TOO_MANY));
        }
    }

    let id = Uuid::now_v7();
    let code = six_digits()?;
    sqlx::query(
        "insert into email_codes (id, email, purpose, user_id, code_hash, ip, expires_at, consumed_at)
         values ($1, $2::citext, $3, $4, $5, $6::inet, now() + interval '10 minutes',
                 case when $7 then now() end)",
    )
    .bind(id)
    .bind(email)
    .bind(purpose)
    .bind(user)
    .bind(seal(&api.code_key, id, &code)?.finalize().into_bytes().to_vec())
    .bind(ip)
    .bind(used)
    .execute(&api.db)
    .await?;
    Ok((id, code))
}

async fn send(api: &Api, email: &str, subject: &str, body: &str) -> Res<()> {
    api.mail.send(email, subject, body).await.map_err(|e| {
        log::error!("{e}");
        ApiError::new(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "mail_failed",
            "The email couldn't be sent. Try again in a minute",
        )
    })
}

/// Checks `code` against the newest live code for `by` and `purpose`, and uses it up when it
/// matches. A wrong code counts as one try, and a code stops working after five.
pub(crate) async fn redeem(api: &Api, purpose: Purpose, by: By<'_>, code: &str) -> Res<Redeemed> {
    let wrong = || ApiError::bad_request("invalid_code", "That code is wrong or has expired");
    let query = match by {
        By::Email(_) => concat!(
            "select id, email::text, user_id, code_hash, attempts from email_codes
             where consumed_at is null and expires_at > now() and purpose = $1",
            " and email = $2::citext order by created_at desc limit 1"
        ),
        By::User(_) => concat!(
            "select id, email::text, user_id, code_hash, attempts from email_codes
             where consumed_at is null and expires_at > now() and purpose = $1",
            " and user_id = $2 order by created_at desc limit 1"
        ),
        By::Id(_) => concat!(
            "select id, email::text, user_id, code_hash, attempts from email_codes
             where consumed_at is null and expires_at > now() and purpose = $1",
            " and id = $2"
        ),
    };
    let query =
        sqlx::query_as::<_, (Uuid, String, Option<Uuid>, Vec<u8>, i32)>(query).bind(purpose);
    let query = match by {
        By::Email(email) => query.bind(email.to_owned()),
        By::User(id) | By::Id(id) => query.bind(id),
    };
    let Some((id, email, user_id, hash, attempts)) = query.fetch_optional(&api.db).await? else {
        return Err(wrong());
    };
    if attempts >= ATTEMPTS {
        return Err(wrong());
    }
    if seal(&api.code_key, id, code.trim())?
        .verify_slice(&hash)
        .is_err()
    {
        sqlx::query("update email_codes set attempts = attempts + 1 where id = $1")
            .bind(id)
            .execute(&api.db)
            .await?;
        return Err(wrong());
    }
    // Only one request can use a code up, even when two arrive at once.
    let used = sqlx::query(
        "update email_codes set consumed_at = now() where id = $1 and consumed_at is null",
    )
    .bind(id)
    .execute(&api.db)
    .await?;
    if used.rows_affected() != 1 {
        return Err(wrong());
    }
    Ok(Redeemed { email, user_id })
}

/// The HMAC of a code, bound to its row so one code can't stand in for another.
fn seal(key: &[u8; 32], id: Uuid, code: &str) -> Res<Hmac<Sha256>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| ApiError::internal())?;
    mac.update(id.as_bytes());
    mac.update(code.as_bytes());
    Ok(mac)
}

fn six_digits() -> Res<String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|e| {
        log::error!("no randomness: {e}");
        ApiError::internal()
    })?;
    Ok(format!("{:06}", u64::from_le_bytes(bytes) % 1_000_000))
}
