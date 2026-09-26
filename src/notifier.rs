use std::env;

use async_trait::async_trait;
use chrono::{DateTime, Local, Utc};
use lettre::{
    message::Mailbox, transport::smtp::authentication::Credentials, AsyncSmtpTransport,
    AsyncTransport, Message, Tokio1Executor,
};
use thiserror::Error;

use crate::{AlertReason, LineupAlert, MonitorAlert};

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
    async fn notify(&self, alerts: &[MonitorAlert]) -> Result<(), NotifierError>;
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
    async fn notify(&self, alerts: &[MonitorAlert]) -> Result<(), NotifierError> {
        if alerts.is_empty() {
            return Ok(());
        }

        let email = Message::builder()
            .from(self.config.from.clone())
            .to(self.config.to.clone())
            .subject(format!("Fantasy lineup alert: {} item(s)", alerts.len()))
            .body(format_alert_email(alerts))?;
        self.send(email).await
    }
}

pub fn format_alert_email(alerts: &[MonitorAlert]) -> String {
    let mut body = String::from("Your fantasy lineup needs attention:\n");
    for alert in alerts {
        match alert {
            MonitorAlert::Starter(alert) => format_starter_alert(&mut body, alert),
            MonitorAlert::RecommendedLineup(recommendation) => {
                body.push_str(&format!(
                    "\n- {} / {}: a valid unlocked lineup improves {} by {:.1} points ({:.1} to {:.1}).\n",
                    recommendation.league_name,
                    recommendation.team_name,
                    projection_label(recommendation.projections_complete),
                    recommendation.projected_gain,
                    recommendation.current_projected_points,
                    recommendation.optimized_projected_points,
                ));
                append_deadline(&mut body, recommendation.action_by);
                for assignment in &recommendation.assignments {
                    body.push_str(&format!(
                        "  {}: {} ({})\n",
                        assignment.slot_name,
                        assignment.player_name,
                        format_projection(assignment.projected_points)
                    ));
                }
            }
            MonitorAlert::RecommendedFreeAgentLineup(recommendation) => {
                let lineup = &recommendation.lineup;
                body.push_str(&format!(
                    "\n- {} / {}: add {} ({:.1}) and drop {} for a valid unlocked lineup that improves {} by {:.1} points ({:.1} to {:.1}).\n",
                    lineup.league_name,
                    lineup.team_name,
                    recommendation.add_player_name,
                    recommendation.add_projected_points,
                    recommendation.drop_player_name,
                    projection_label(lineup.projections_complete),
                    lineup.projected_gain,
                    lineup.current_projected_points,
                    lineup.optimized_projected_points,
                ));
                append_deadline(&mut body, lineup.action_by);
                for assignment in &lineup.assignments {
                    body.push_str(&format!(
                        "  {}: {} ({})\n",
                        assignment.slot_name,
                        assignment.player_name,
                        format_projection(assignment.projected_points)
                    ));
                }
            }
        }
    }
    body
}

fn append_deadline(body: &mut String, deadline: Option<DateTime<Utc>>) {
    if let Some(deadline) = deadline {
        body.push_str(&format!(
            "  Act by: {}\n",
            deadline
                .with_timezone(&Local)
                .format("%a, %b %-d at %-I:%M %p %Z")
        ));
    }
}

fn projection_label(projections_complete: bool) -> &'static str {
    if projections_complete {
        "projected points"
    } else {
        "known projected points"
    }
}

fn format_projection(projection: Option<f64>) -> String {
    projection
        .map(|points| format!("{points:.1}"))
        .unwrap_or_else(|| "projection unavailable".to_owned())
}

fn format_starter_alert(body: &mut String, alert: &LineupAlert) {
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
    use crate::{
        FreeAgentRecommendation, LineupAlert, LineupAssignment, LineupRecommendation, MonitorAlert,
        Provider,
    };
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn formats_actionable_alerts() {
        let body = format_alert_email(&[MonitorAlert::Starter(LineupAlert {
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
        })]);

        assert!(body.contains("Sunday League / My Team"));
        assert!(body.contains("Player One (WR) [unavailable, zero projection]"));
    }

    #[test]
    fn formats_free_agent_recommendations() {
        let body = format_alert_email(&[MonitorAlert::RecommendedFreeAgentLineup(
            FreeAgentRecommendation {
                lineup: LineupRecommendation {
                    provider: Provider::Sleeper,
                    league_id: "1".to_owned(),
                    league_name: "Sunday League".to_owned(),
                    team_id: "2".to_owned(),
                    team_name: "My Team".to_owned(),
                    current_projected_points: 90.0,
                    optimized_projected_points: 96.0,
                    projected_gain: 6.0,
                    projections_complete: true,
                    action_by: Some(Utc.with_ymd_and_hms(2026, 9, 27, 17, 0, 0).unwrap()),
                    assignments: vec![LineupAssignment {
                        slot_name: "WR".to_owned(),
                        player_id: "3".to_owned(),
                        player_name: "New Player".to_owned(),
                        projected_points: Some(14.0),
                    }],
                },
                add_player_id: "3".to_owned(),
                add_player_name: "New Player".to_owned(),
                add_projected_points: 14.0,
                drop_player_id: "4".to_owned(),
                drop_player_name: "Old Player".to_owned(),
            },
        )]);

        assert!(body.contains("add New Player (14.0) and drop Old Player"));
        assert!(body.contains("Act by:"));
        assert!(body.contains("WR: New Player (14.0)"));
    }
}
