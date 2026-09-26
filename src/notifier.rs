use std::env;

use async_trait::async_trait;
use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use thiserror::Error;

use crate::{AlertReason, LineupAlert};

#[derive(Debug, Error)]
pub enum NotifierError {
    #[error("{0} is required for email alerts")]
    MissingSetting(&'static str),
    #[error("EMAIL_SMTP_PORT must be a valid port: {0}")]
    InvalidPort(#[from] std::num::ParseIntError),
    #[error("EMAIL_SMTP_SECURITY must be `implicit` or `starttls`")]
    InvalidSecurity,
    #[error(transparent)]
    Address(#[from] lettre::address::AddressError),
    #[error(transparent)]
    Message(#[from] lettre::error::Error),
    #[error(transparent)]
    Smtp(#[from] lettre::transport::smtp::Error),
}

#[async_trait]
pub trait AlertNotifier: Send + Sync {
    async fn notify(&self, alerts: &[LineupAlert]) -> Result<(), NotifierError>;
}

pub struct EmailNotifier {
    config: EmailConfig,
}

impl EmailNotifier {
    pub fn from_env() -> Result<Self, NotifierError> {
        Ok(Self {
            config: EmailConfig::from_env()?,
        })
    }

    pub async fn send_test_email(&self) -> Result<(), NotifierError> {
        let email = Message::builder()
            .from(self.config.from.clone())
            .to(self.config.to.clone())
            .subject("Fantasy Monitor email delivery test")
            .body(
                "This is a test email from Fantasy Monitor. No roster action is required."
                    .to_owned(),
            )?;
        self.send(email).await
    }

    pub async fn send_espn_session_refresh_email(&self) -> Result<(), NotifierError> {
        let email = Message::builder()
            .from(self.config.from.clone())
            .to(self.config.to.clone())
            .subject("Fantasy Monitor needs a refreshed ESPN session")
            .body(
                "ESPN rejected the saved session, so Fantasy Monitor could not check your ESPN leagues.\n\n\
Sign in at fantasy.espn.com, open browser developer tools, and copy the espn_s2 cookie from the fantasy.espn.com site storage. Replace ESPN_S2 in your local .env file with that value.\n\n\
The next scheduled check will confirm that monitoring has recovered."
                    .to_owned(),
            )?;
        self.send(email).await
    }

    pub async fn send_espn_session_recovered_email(&self) -> Result<(), NotifierError> {
        let email = Message::builder()
            .from(self.config.from.clone())
            .to(self.config.to.clone())
            .subject("Fantasy Monitor ESPN session restored")
            .body(
                "Fantasy Monitor successfully read your ESPN leagues again. Regular roster monitoring has resumed."
                    .to_owned(),
            )?;
        self.send(email).await
    }

    async fn send(&self, email: Message) -> Result<(), NotifierError> {
        let credentials =
            Credentials::new(self.config.username.clone(), self.config.password.clone());
        let builder = match self.config.security {
            SmtpSecurity::Implicit => {
                AsyncSmtpTransport::<Tokio1Executor>::relay(&self.config.host)?
            }
            SmtpSecurity::StartTls => {
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.config.host)?
            }
        };
        let mailer = builder
            .port(self.config.port)
            .credentials(credentials)
            .build();

        mailer.send(email).await?;
        Ok(())
    }
}

#[async_trait]
impl AlertNotifier for EmailNotifier {
    async fn notify(&self, alerts: &[LineupAlert]) -> Result<(), NotifierError> {
        if alerts.is_empty() {
            return Ok(());
        }

        let email = Message::builder()
            .from(self.config.from.clone())
            .to(self.config.to.clone())
            .subject(format!("Fantasy lineup alert: {} starter(s)", alerts.len()))
            .body(format_alert_email(alerts))?;
        self.send(email).await
    }
}

pub fn format_alert_email(alerts: &[LineupAlert]) -> String {
    let mut body = String::from("Your fantasy lineup needs attention:\n");
    for alert in alerts {
        body.push_str(&format!(
            "\n- {} / {}: {} ({}) [{}]\n",
            alert.league_name,
            alert.team_name,
            alert.player_name,
            alert.position.as_deref().unwrap_or("unknown position"),
            alert
                .reasons
                .iter()
                .map(alert_reason_name)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    body
}

struct EmailConfig {
    host: String,
    port: u16,
    security: SmtpSecurity,
    username: String,
    password: String,
    from: Mailbox,
    to: Mailbox,
}

impl EmailConfig {
    fn from_env() -> Result<Self, NotifierError> {
        Ok(Self {
            host: setting("EMAIL_SMTP_HOST")?,
            port: env::var("EMAIL_SMTP_PORT")
                .unwrap_or_else(|_| "465".to_owned())
                .parse()?,
            security: SmtpSecurity::from_env()?,
            username: setting("EMAIL_SMTP_USERNAME")?,
            password: setting("EMAIL_SMTP_PASSWORD")?,
            from: setting("EMAIL_FROM")?.parse()?,
            to: setting("EMAIL_TO")?.parse()?,
        })
    }
}

#[derive(Clone, Copy)]
enum SmtpSecurity {
    Implicit,
    StartTls,
}

impl SmtpSecurity {
    fn from_env() -> Result<Self, NotifierError> {
        match env::var("EMAIL_SMTP_SECURITY")
            .unwrap_or_else(|_| "implicit".to_owned())
            .to_ascii_lowercase()
            .as_str()
        {
            "implicit" => Ok(Self::Implicit),
            "starttls" => Ok(Self::StartTls),
            _ => Err(NotifierError::InvalidSecurity),
        }
    }
}

fn setting(name: &'static str) -> Result<String, NotifierError> {
    env::var(name).map_err(|_| NotifierError::MissingSetting(name))
}

fn alert_reason_name(reason: &AlertReason) -> &'static str {
    match reason {
        AlertReason::ByeWeek => "bye week",
        AlertReason::ConfirmedUnavailable => "unavailable",
        AlertReason::ZeroProjection => "zero projection",
    }
}

#[cfg(test)]
mod tests {
    use crate::{LineupAlert, Provider};

    use super::*;

    #[test]
    fn formats_actionable_alerts() {
        let body = format_alert_email(&[LineupAlert {
            provider: Provider::Espn,
            league_id: "1".to_owned(),
            league_name: "Sunday League".to_owned(),
            team_id: "2".to_owned(),
            team_name: "My Team".to_owned(),
            player_id: "3".to_owned(),
            player_name: "Player One".to_owned(),
            position: Some("WR".to_owned()),
            reasons: vec![
                AlertReason::ConfirmedUnavailable,
                AlertReason::ZeroProjection,
            ],
        }]);

        assert!(body.contains("Sunday League / My Team"));
        assert!(body.contains("Player One (WR) [unavailable, zero projection]"));
    }
}
