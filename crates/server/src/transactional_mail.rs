use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use chrono::{DateTime, Utc};
use clap::Args;
use futures::{StreamExt, TryStreamExt};
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart, SinglePart, header::ContentType},
    transport::smtp::{
        authentication::Credentials,
        client::{Tls, TlsParameters},
    },
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use thiserror::Error;
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

pub const MAX_RECIPIENTS: usize = 100;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    En,
    Ru,
}

impl Locale {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ru => "ru",
        }
    }
}

impl std::str::FromStr for Locale {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "en" => Ok(Self::En),
            "ru" => Ok(Self::Ru),
            _ => Err("locale must be en or ru"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmtpTlsMode {
    StartTls,
    Implicit,
    Plaintext,
}

impl std::str::FromStr for SmtpTlsMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "starttls" => Ok(Self::StartTls),
            "implicit" => Ok(Self::Implicit),
            "plaintext" => Ok(Self::Plaintext),
            _ => Err("SMTP TLS mode must be starttls, implicit, or plaintext"),
        }
    }
}

#[derive(Clone, Args)]
pub struct MailArgs {
    #[arg(
        id = "mail_enabled",
        long = "mail-enabled",
        env = "OKOSCOPE_MAIL_ENABLED",
        default_value_t = false
    )]
    pub enabled: bool,
    #[arg(
        id = "mail_public_web_url",
        long = "mail-public-web-url",
        env = "OKOSCOPE_PUBLIC_WEB_URL"
    )]
    pub public_web_url: Option<String>,
    #[arg(
        id = "mail_smtp_host",
        long = "mail-smtp-host",
        env = "OKOSCOPE_SMTP_HOST"
    )]
    pub smtp_host: Option<String>,
    #[arg(
        id = "mail_smtp_port",
        long = "mail-smtp-port",
        env = "OKOSCOPE_SMTP_PORT",
        default_value_t = 587
    )]
    pub smtp_port: u16,
    #[arg(
        id = "mail_smtp_tls",
        long = "mail-smtp-tls",
        env = "OKOSCOPE_SMTP_TLS",
        default_value = "starttls"
    )]
    pub smtp_tls: String,
    #[arg(
        id = "mail_smtp_username",
        long = "mail-smtp-username",
        env = "OKOSCOPE_SMTP_USERNAME",
        hide_env_values = true
    )]
    pub smtp_username: Option<String>,
    #[arg(
        id = "mail_smtp_password",
        long = "mail-smtp-password",
        env = "OKOSCOPE_SMTP_PASSWORD",
        hide_env_values = true
    )]
    pub smtp_password: Option<String>,
    #[arg(
        id = "mail_from_address",
        long = "mail-from-address",
        env = "OKOSCOPE_MAIL_FROM_ADDRESS"
    )]
    pub from_address: Option<String>,
    #[arg(
        id = "mail_from_name",
        long = "mail-from-name",
        env = "OKOSCOPE_MAIL_FROM_NAME",
        default_value = "Okoscope"
    )]
    pub from_name: String,
    #[arg(
        id = "mail_default_locale",
        long = "mail-default-locale",
        env = "OKOSCOPE_MAIL_DEFAULT_LOCALE",
        default_value = "en"
    )]
    pub default_locale: String,
    #[arg(
        id = "mail_encryption_key",
        long = "mail-encryption-key",
        env = "OKOSCOPE_MAIL_ENCRYPTION_KEY",
        hide_env_values = true
    )]
    pub encryption_key: Option<String>,
    #[arg(
        id = "mail_poll_ms",
        long = "mail-poll-ms",
        env = "OKOSCOPE_MAIL_POLL_MS",
        default_value_t = 1000
    )]
    pub poll_ms: u64,
    #[arg(
        id = "mail_claim_size",
        long = "mail-claim-size",
        env = "OKOSCOPE_MAIL_CLAIM_SIZE",
        default_value_t = 25
    )]
    pub claim_size: u32,
    #[arg(
        id = "mail_concurrency",
        long = "mail-concurrency",
        env = "OKOSCOPE_MAIL_CONCURRENCY",
        default_value_t = 4
    )]
    pub concurrency: usize,
    #[arg(
        id = "mail_lease_seconds",
        long = "mail-lease-seconds",
        env = "OKOSCOPE_MAIL_LEASE_SECONDS",
        default_value_t = 60
    )]
    pub lease_seconds: u64,
    #[arg(
        id = "mail_timeout_seconds",
        long = "mail-timeout-seconds",
        env = "OKOSCOPE_SMTP_TIMEOUT_SECONDS",
        default_value_t = 15
    )]
    pub timeout_seconds: u64,
    #[arg(
        id = "mail_max_attempts",
        long = "mail-max-attempts",
        env = "OKOSCOPE_MAIL_MAX_ATTEMPTS",
        default_value_t = 8
    )]
    pub max_attempts: u32,
    #[arg(
        id = "mail_retention_days",
        long = "mail-retention-days",
        env = "OKOSCOPE_MAIL_RETENTION_DAYS",
        default_value_t = 30
    )]
    pub retention_days: u64,
}

impl std::fmt::Debug for MailArgs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MailArgs")
            .field("enabled", &self.enabled)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_tls", &self.smtp_tls)
            .field("claim_size", &self.claim_size)
            .field("concurrency", &self.concurrency)
            .finish_non_exhaustive()
    }
}

#[derive(Clone)]
pub struct MailConfig {
    pub enabled: bool,
    pub public_web_url: Url,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_tls: SmtpTlsMode,
    pub smtp_username: String,
    pub smtp_password: Zeroizing<String>,
    pub from: Mailbox,
    pub default_locale: Locale,
    pub encryption_key: [u8; 32],
    pub poll_interval: Duration,
    pub claim_size: u32,
    pub concurrency: usize,
    pub lease: Duration,
    pub timeout: Duration,
    pub max_attempts: u32,
    pub retention: Duration,
}

impl std::fmt::Debug for MailConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MailConfig")
            .field("enabled", &self.enabled)
            .field("smtp_tls", &self.smtp_tls)
            .field("claim_size", &self.claim_size)
            .field("concurrency", &self.concurrency)
            .finish_non_exhaustive()
    }
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            public_web_url: Url::parse("http://localhost").expect("static URL"),
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_tls: SmtpTlsMode::StartTls,
            smtp_username: String::new(),
            smtp_password: Zeroizing::new(String::new()),
            from: "Okoscope <noreply@example.invalid>"
                .parse()
                .expect("static mailbox"),
            default_locale: Locale::En,
            encryption_key: [0; 32],
            poll_interval: Duration::from_secs(1),
            claim_size: 25,
            concurrency: 4,
            lease: Duration::from_secs(60),
            timeout: Duration::from_secs(15),
            max_attempts: 8,
            retention: Duration::from_secs(30 * 86_400),
        }
    }
}

impl MailArgs {
    pub fn build(&self, development: bool, registration: bool) -> Result<MailConfig, String> {
        if !self.enabled {
            if registration {
                return Err("public registration requires transactional mail".into());
            }
            return Ok(MailConfig::default());
        }
        let public_web_url = validate_public_url(self.public_web_url.as_deref(), development)?;
        let smtp_tls: SmtpTlsMode = self.smtp_tls.parse().map_err(str::to_owned)?;
        if smtp_tls == SmtpTlsMode::Plaintext && !development {
            return Err("plaintext SMTP is allowed only in development mode".into());
        }
        validate_bounds(self)?;
        let smtp_host = required(&self.smtp_host, "SMTP host")?;
        let smtp_username = required(&self.smtp_username, "SMTP username")?;
        let smtp_password = Zeroizing::new(required(&self.smtp_password, "SMTP password")?);
        let from_address = required(&self.from_address, "mail sender address")?;
        reject_header_injection(&self.from_name)?;
        let from = format!("{} <{}>", self.from_name, from_address)
            .parse()
            .map_err(|_| "mail sender address is invalid".to_owned())?;
        let default_locale = self.default_locale.parse().map_err(str::to_owned)?;
        let encryption_key = decode_key(self.encryption_key.as_deref())?;
        Ok(MailConfig {
            enabled: true,
            public_web_url,
            smtp_host,
            smtp_port: self.smtp_port,
            smtp_tls,
            smtp_username,
            smtp_password,
            from,
            default_locale,
            encryption_key,
            poll_interval: Duration::from_millis(self.poll_ms),
            claim_size: self.claim_size,
            concurrency: self.concurrency,
            lease: Duration::from_secs(self.lease_seconds),
            timeout: Duration::from_secs(self.timeout_seconds),
            max_attempts: self.max_attempts,
            retention: Duration::from_secs(self.retention_days * 86_400),
        })
    }
}

fn required(value: &Option<String>, name: &str) -> Result<String, String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{name} is required when mail is enabled"))
}

fn validate_public_url(value: Option<&str>, development: bool) -> Result<Url, String> {
    let url = Url::parse(value.ok_or("public Web URL is required when mail is enabled")?)
        .map_err(|_| "public Web URL is invalid".to_owned())?;
    let valid = url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (url.scheme() == "https" || development && url.scheme() == "http");
    if !valid {
        return Err("public Web URL must be an HTTPS origin".into());
    }
    Ok(url)
}

fn validate_bounds(args: &MailArgs) -> Result<(), String> {
    if args.smtp_port == 0
        || !(100..=60_000).contains(&args.poll_ms)
        || !(1..=100).contains(&args.claim_size)
        || !(1..=32).contains(&args.concurrency)
        || !(10..=600).contains(&args.lease_seconds)
        || !(1..=120).contains(&args.timeout_seconds)
        || !(1..=20).contains(&args.max_attempts)
        || !(1..=365).contains(&args.retention_days)
    {
        return Err("mail worker configuration is outside supported bounds".into());
    }
    Ok(())
}

fn decode_key(value: Option<&str>) -> Result<[u8; 32], String> {
    let bytes = hex::decode(value.ok_or("mail encryption key is required")?)
        .map_err(|_| "mail encryption key must be 64 hexadecimal characters".to_owned())?;
    bytes
        .try_into()
        .map_err(|_| "mail encryption key must be 64 hexadecimal characters".into())
}

fn reject_header_injection(value: &str) -> Result<(), String> {
    if value.contains(['\r', '\n']) || value.trim().is_empty() || value.chars().count() > 120 {
        return Err("mail header value is invalid".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateData {
    VerifyEmail {
        action_url: String,
        organization_name: String,
        expires_minutes: i64,
    },
    ResetPassword {
        action_url: String,
        expires_minutes: i64,
    },
    PasswordChanged,
    ApplicationCreated {
        application_name: String,
        project_name: String,
    },
}

impl TemplateData {
    fn kind(&self) -> &'static str {
        match self {
            Self::VerifyEmail { .. } => "verify_email",
            Self::ResetPassword { .. } => "reset_password",
            Self::PasswordChanged => "password_changed",
            Self::ApplicationCreated { .. } => "application_created",
        }
    }
}

#[derive(Debug, Error)]
pub enum MailError {
    #[error("transactional mail recipient set exceeds the supported limit")]
    TooManyRecipients,
    #[error("transactional mail payload is invalid")]
    InvalidPayload,
    #[error("transactional mail persistence failed")]
    Database(#[from] sqlx::Error),
}

pub async fn enqueue(
    tx: &mut Transaction<'_, Postgres>,
    config: &MailConfig,
    logical_key: &str,
    recipients: &[(String, Locale)],
    data: &TemplateData,
    action_id: Option<Uuid>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(), MailError> {
    if !config.enabled || recipients.is_empty() {
        return Ok(());
    }
    let recipients = deduplicate(recipients)?;
    let serialized = serde_json::to_vec(data).map_err(|_| MailError::InvalidPayload)?;
    if serialized.len() > 16_384 || logical_key.len() > 256 {
        return Err(MailError::InvalidPayload);
    }
    for (recipient, locale) in recipients {
        insert_encrypted(
            tx,
            config,
            logical_key,
            &recipient,
            locale,
            data.kind(),
            &serialized,
            action_id,
            expires_at,
        )
        .await?;
    }
    Ok(())
}

fn deduplicate(recipients: &[(String, Locale)]) -> Result<Vec<(String, Locale)>, MailError> {
    if recipients.len() > MAX_RECIPIENTS {
        return Err(MailError::TooManyRecipients);
    }
    let mut result = Vec::new();
    for (email, locale) in recipients {
        let email = crate::auth::normalize_email(email).map_err(|_| MailError::InvalidPayload)?;
        if !result.iter().any(|(existing, _)| existing == &email) {
            result.push((email, *locale));
        }
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn insert_encrypted(
    tx: &mut Transaction<'_, Postgres>,
    config: &MailConfig,
    logical_key: &str,
    recipient: &str,
    locale: Locale,
    kind: &str,
    serialized: &[u8],
    action_id: Option<Uuid>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<(), MailError> {
    let id = Uuid::new_v4();
    let (ciphertext, nonce) =
        encrypt_payload(&config.encryption_key, id, kind, recipient, serialized)?;
    let retention =
        chrono::Duration::from_std(config.retention).map_err(|_| MailError::InvalidPayload)?;
    sqlx::query("INSERT INTO transactional_mail_outbox(id,logical_key,template_kind,recipient_email,locale,payload_ciphertext,payload_nonce,action_id,expires_at,retain_until) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,now()+$10) ON CONFLICT(logical_key,recipient_email) DO NOTHING")
        .bind(id).bind(logical_key).bind(kind).bind(recipient).bind(locale.as_str())
        .bind(ciphertext).bind(nonce.to_vec()).bind(action_id).bind(expires_at).bind(retention)
        .execute(&mut **tx).await?;
    Ok(())
}

fn encrypt_payload(
    key: &[u8; 32],
    id: Uuid,
    kind: &str,
    recipient: &str,
    plaintext: &[u8],
) -> Result<(Vec<u8>, [u8; 24]), MailError> {
    let mut nonce = [0_u8; 24];
    rand::rng().fill_bytes(&mut nonce);
    let aad = format!("{id}:{kind}:{recipient}");
    let ciphertext = XChaCha20Poly1305::new(key.into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| MailError::InvalidPayload)?;
    Ok((ciphertext, nonce))
}

#[derive(Clone, Debug)]
pub struct RenderedMail {
    pub subject: String,
    pub text: String,
    pub html: String,
}

pub fn render(locale: Locale, data: &TemplateData) -> RenderedMail {
    match locale {
        Locale::En => render_en(data),
        Locale::Ru => render_ru(data),
    }
}

fn render_en(data: &TemplateData) -> RenderedMail {
    let e = |value: &str| escape_html(value);
    match data {
        TemplateData::VerifyEmail {
            action_url,
            organization_name,
            expires_minutes,
        } => rendered(
            "Confirm your Okoscope email",
            &format!(
                "Welcome to Okoscope. Confirm your email for {organization_name}: {action_url}\nThis link expires in {expires_minutes} minutes. If you did not register, ignore this message."
            ),
            &format!(
                "<h1>Welcome to Okoscope</h1><p>Confirm your email for <strong>{}</strong>.</p><p><a href=\"{}\">Confirm email</a></p><p>This link expires in {expires_minutes} minutes. If you did not register, ignore this message.</p>",
                e(organization_name),
                e(action_url)
            ),
        ),
        TemplateData::ResetPassword {
            action_url,
            expires_minutes,
        } => rendered(
            "Reset your Okoscope password",
            &format!(
                "Reset your password: {action_url}\nThis one-time link expires in {expires_minutes} minutes. If you did not request it, ignore this message."
            ),
            &format!(
                "<h1>Reset your password</h1><p><a href=\"{}\">Choose a new password</a></p><p>This one-time link expires in {expires_minutes} minutes. If you did not request it, ignore this message.</p>",
                e(action_url)
            ),
        ),
        TemplateData::PasswordChanged => rendered(
            "Your Okoscope password changed",
            "Your Okoscope password was changed. If this was not you, contact your administrator.",
            "<h1>Password changed</h1><p>Your Okoscope password was changed. If this was not you, contact your administrator.</p>",
        ),
        TemplateData::ApplicationCreated {
            application_name,
            project_name,
        } => rendered(
            "Okoscope Application created",
            &format!("Application {application_name} was created in Project {project_name}."),
            &format!(
                "<h1>Application created</h1><p><strong>{}</strong> was created in Project <strong>{}</strong>.</p>",
                e(application_name),
                e(project_name)
            ),
        ),
    }
}

fn render_ru(data: &TemplateData) -> RenderedMail {
    let e = |value: &str| escape_html(value);
    match data {
        TemplateData::VerifyEmail {
            action_url,
            organization_name,
            expires_minutes,
        } => rendered(
            "Подтвердите почту Okoscope",
            &format!(
                "Добро пожаловать в Okoscope. Подтвердите почту для {organization_name}: {action_url}\nСсылка действует {expires_minutes} минут. Если вы не регистрировались, проигнорируйте письмо."
            ),
            &format!(
                "<h1>Добро пожаловать в Okoscope</h1><p>Подтвердите почту для <strong>{}</strong>.</p><p><a href=\"{}\">Подтвердить почту</a></p><p>Ссылка действует {expires_minutes} минут. Если вы не регистрировались, проигнорируйте письмо.</p>",
                e(organization_name),
                e(action_url)
            ),
        ),
        TemplateData::ResetPassword {
            action_url,
            expires_minutes,
        } => rendered(
            "Сброс пароля Okoscope",
            &format!(
                "Задайте новый пароль: {action_url}\nОдноразовая ссылка действует {expires_minutes} минут. Если вы не запрашивали сброс, проигнорируйте письмо."
            ),
            &format!(
                "<h1>Сброс пароля</h1><p><a href=\"{}\">Задать новый пароль</a></p><p>Одноразовая ссылка действует {expires_minutes} минут. Если вы не запрашивали сброс, проигнорируйте письмо.</p>",
                e(action_url)
            ),
        ),
        TemplateData::PasswordChanged => rendered(
            "Пароль Okoscope изменён",
            "Ваш пароль Okoscope изменён. Если это были не вы, обратитесь к администратору.",
            "<h1>Пароль изменён</h1><p>Ваш пароль Okoscope изменён. Если это были не вы, обратитесь к администратору.</p>",
        ),
        TemplateData::ApplicationCreated {
            application_name,
            project_name,
        } => rendered(
            "Создано приложение Okoscope",
            &format!("Приложение {application_name} создано в проекте {project_name}."),
            &format!(
                "<h1>Создано приложение</h1><p><strong>{}</strong> создано в проекте <strong>{}</strong>.</p>",
                e(application_name),
                e(project_name)
            ),
        ),
    }
}

fn rendered(subject: &str, text: &str, html: &str) -> RenderedMail {
    RenderedMail {
        subject: subject.to_owned(),
        text: text.to_owned(),
        html: html.to_owned(),
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeliveryFailure {
    Transient,
    Permanent,
}

pub trait MailSender: Send + Sync {
    fn send<'a>(
        &'a self,
        recipient: &'a str,
        rendered: &'a RenderedMail,
    ) -> Pin<Box<dyn Future<Output = Result<(), DeliveryFailure>> + Send + 'a>>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedMail {
    pub recipient: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

#[derive(Debug, Default)]
pub struct CapturingSender {
    messages: std::sync::Mutex<Vec<CapturedMail>>,
}

impl CapturingSender {
    pub fn messages(&self) -> Vec<CapturedMail> {
        self.messages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl MailSender for CapturingSender {
    fn send<'a>(
        &'a self,
        recipient: &'a str,
        rendered: &'a RenderedMail,
    ) -> Pin<Box<dyn Future<Output = Result<(), DeliveryFailure>> + Send + 'a>> {
        Box::pin(async move {
            self.messages
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(CapturedMail {
                    recipient: recipient.to_owned(),
                    subject: rendered.subject.clone(),
                    text: rendered.text.clone(),
                    html: rendered.html.clone(),
                });
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct SmtpSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl std::fmt::Debug for SmtpSender {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("SmtpSender").finish_non_exhaustive()
    }
}

impl SmtpSender {
    pub fn new(config: &MailConfig) -> Result<Self, String> {
        let tls_parameters = TlsParameters::new(config.smtp_host.clone())
            .map_err(|_| "SMTP TLS parameters are invalid".to_owned())?;
        let tls = match config.smtp_tls {
            SmtpTlsMode::StartTls => Tls::Required(tls_parameters.clone()),
            SmtpTlsMode::Implicit => Tls::Wrapper(tls_parameters),
            SmtpTlsMode::Plaintext => Tls::None,
        };
        let transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
            .port(config.smtp_port)
            .tls(tls)
            .credentials(Credentials::new(
                config.smtp_username.clone(),
                config.smtp_password.to_string(),
            ))
            .timeout(Some(config.timeout))
            .build();
        Ok(Self {
            transport,
            from: config.from.clone(),
        })
    }
}

impl MailSender for SmtpSender {
    fn send<'a>(
        &'a self,
        recipient: &'a str,
        rendered: &'a RenderedMail,
    ) -> Pin<Box<dyn Future<Output = Result<(), DeliveryFailure>> + Send + 'a>> {
        Box::pin(async move {
            let to: Mailbox = recipient.parse().map_err(|_| DeliveryFailure::Permanent)?;
            let message = Message::builder()
                .from(self.from.clone())
                .to(to)
                .subject(&rendered.subject)
                .multipart(
                    MultiPart::alternative()
                        .singlepart(
                            SinglePart::builder()
                                .header(ContentType::TEXT_PLAIN)
                                .body(rendered.text.clone()),
                        )
                        .singlepart(
                            SinglePart::builder()
                                .header(ContentType::TEXT_HTML)
                                .body(rendered.html.clone()),
                        ),
                )
                .map_err(|_| DeliveryFailure::Permanent)?;
            self.transport
                .send(message)
                .await
                .map(|_| ())
                .map_err(|error| classify_smtp(&error))
        })
    }
}

fn classify_smtp(error: &lettre::transport::smtp::Error) -> DeliveryFailure {
    if error.is_permanent() {
        DeliveryFailure::Permanent
    } else {
        DeliveryFailure::Transient
    }
}

#[derive(Debug, FromRow)]
struct ClaimedMail {
    id: Uuid,
    template_kind: String,
    recipient_email: String,
    locale: String,
    payload_ciphertext: Vec<u8>,
    payload_nonce: Vec<u8>,
    attempt_count: i32,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone)]
pub struct MailService {
    pool: PgPool,
    config: MailConfig,
    sender: Arc<dyn MailSender>,
}

impl std::fmt::Debug for MailService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MailService")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl MailService {
    pub fn smtp(pool: PgPool, config: MailConfig) -> Result<Self, String> {
        let sender = Arc::new(SmtpSender::new(&config)?);
        Ok(Self {
            pool,
            config,
            sender,
        })
    }

    pub fn with_sender(pool: PgPool, config: MailConfig, sender: Arc<dyn MailSender>) -> Self {
        Self {
            pool,
            config,
            sender,
        }
    }
}

pub async fn run(service: MailService, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let worker_id = Uuid::new_v4();
    while !*shutdown.borrow() {
        if let Err(error) = process_batch(&service, worker_id).await {
            tracing::error!(error = %error, "transactional mail worker cycle failed");
        }
        tokio::select! {
            () = tokio::time::sleep(service.config.poll_interval) => {},
            result = shutdown.changed() => if result.is_err() { break; },
        }
    }
}

async fn process_batch(service: &MailService, worker_id: Uuid) -> Result<(), MailError> {
    let claimed = claim(&service.pool, &service.config, worker_id).await?;
    crate::metrics::record_mail_claims(claimed.len());
    futures::stream::iter(claimed)
        .map(|mail| process_one(service, worker_id, mail))
        .buffer_unordered(service.config.concurrency)
        .try_collect::<Vec<_>>()
        .await?;
    cleanup(&service.pool).await?;
    Ok(())
}

pub async fn process_once(service: &MailService) -> Result<(), MailError> {
    process_batch(service, Uuid::new_v4()).await
}

async fn claim(
    pool: &PgPool,
    config: &MailConfig,
    worker_id: Uuid,
) -> Result<Vec<ClaimedMail>, sqlx::Error> {
    let lease =
        chrono::Duration::from_std(config.lease).unwrap_or_else(|_| chrono::Duration::seconds(60));
    sqlx::query_as("WITH due AS (SELECT id FROM transactional_mail_outbox WHERE delivered_at IS NULL AND terminal_at IS NULL AND available_at<=now() AND (claimed_until IS NULL OR claimed_until<now()) ORDER BY available_at,created_at,id LIMIT $1 FOR UPDATE SKIP LOCKED) UPDATE transactional_mail_outbox o SET claimed_by=$2,claimed_until=now()+$3,attempt_count=attempt_count+1,last_attempt_at=now() FROM due WHERE o.id=due.id RETURNING o.id,o.template_kind,o.recipient_email,o.locale,o.payload_ciphertext,o.payload_nonce,o.attempt_count,o.expires_at")
        .bind(i64::from(config.claim_size)).bind(worker_id).bind(lease).fetch_all(pool).await
}

async fn process_one(
    service: &MailService,
    worker_id: Uuid,
    mail: ClaimedMail,
) -> Result<(), MailError> {
    if mail.expires_at.is_some_and(|expiry| expiry <= Utc::now()) {
        terminal(&service.pool, mail.id, worker_id, "action_expired").await?;
        return Ok(());
    }
    let data = decrypt(&service.config, &mail)
        .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|_| MailError::InvalidPayload));
    let Ok(data) = data else {
        terminal(&service.pool, mail.id, worker_id, "payload_invalid").await?;
        return Ok(());
    };
    let locale = mail.locale.parse().unwrap_or(Locale::En);
    let rendered = render(locale, &data);
    match service.sender.send(&mail.recipient_email, &rendered).await {
        Ok(()) => {
            delivered(&service.pool, mail.id, worker_id).await?;
            crate::metrics::record_mail_attempt(true, false, false);
        }
        Err(DeliveryFailure::Permanent) => {
            terminal(&service.pool, mail.id, worker_id, "permanent_rejection").await?;
            crate::metrics::record_mail_attempt(false, false, true);
        }
        Err(DeliveryFailure::Transient)
            if u32::try_from(mail.attempt_count).unwrap_or(u32::MAX)
                >= service.config.max_attempts =>
        {
            terminal(&service.pool, mail.id, worker_id, "attempts_exhausted").await?;
            crate::metrics::record_mail_attempt(false, false, true);
        }
        Err(DeliveryFailure::Transient) => {
            retry(&service.pool, mail.id, worker_id, mail.attempt_count).await?;
            crate::metrics::record_mail_attempt(false, true, false);
        }
    }
    Ok(())
}

fn decrypt(config: &MailConfig, mail: &ClaimedMail) -> Result<Zeroizing<Vec<u8>>, MailError> {
    if mail.payload_nonce.len() != 24 {
        return Err(MailError::InvalidPayload);
    }
    let aad = format!(
        "{}:{}:{}",
        mail.id, mail.template_kind, mail.recipient_email
    );
    XChaCha20Poly1305::new((&config.encryption_key).into())
        .decrypt(
            XNonce::from_slice(&mail.payload_nonce),
            Payload {
                msg: &mail.payload_ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| MailError::InvalidPayload)
}

async fn delivered(pool: &PgPool, id: Uuid, worker: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE transactional_mail_outbox SET delivered_at=now(),claimed_by=NULL,claimed_until=NULL,payload_ciphertext=NULL,payload_nonce=NULL,ciphertext_erased_at=now() WHERE id=$1 AND claimed_by=$2")
        .bind(id).bind(worker).execute(pool).await?;
    Ok(())
}

async fn terminal(pool: &PgPool, id: Uuid, worker: Uuid, reason: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE transactional_mail_outbox SET terminal_at=now(),terminal_reason=$3,claimed_by=NULL,claimed_until=NULL,payload_ciphertext=NULL,payload_nonce=NULL,ciphertext_erased_at=now() WHERE id=$1 AND claimed_by=$2")
        .bind(id).bind(worker).bind(reason).execute(pool).await?;
    Ok(())
}

async fn retry(pool: &PgPool, id: Uuid, worker: Uuid, attempt: i32) -> Result<(), sqlx::Error> {
    let exponent = u32::try_from(attempt.clamp(1, 10)).unwrap_or(10);
    let seconds = 5_i64
        .saturating_mul(2_i64.saturating_pow(exponent))
        .min(3600);
    sqlx::query("UPDATE transactional_mail_outbox SET available_at=now()+make_interval(secs=>$3),claimed_by=NULL,claimed_until=NULL WHERE id=$1 AND claimed_by=$2")
        .bind(id).bind(worker).bind(f64::from(i32::try_from(seconds).unwrap_or(3600))).execute(pool).await?;
    Ok(())
}

pub async fn cleanup(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM user_email_actions WHERE (expires_at<now() OR consumed_at IS NOT NULL OR revoked_at IS NOT NULL) AND created_at<now()-interval '30 days'").execute(pool).await?;
    sqlx::query("DELETE FROM transactional_mail_outbox WHERE retain_until<now() AND (delivered_at IS NOT NULL OR terminal_at IS NOT NULL)").execute(pool).await?;
    sqlx::query("WITH candidates AS (SELECT u.id user_id,m.organization_id FROM users u JOIN organization_memberships m ON m.user_id=u.id AND m.role='owner' WHERE u.email_verified_at IS NULL AND u.created_at<now()-interval '7 days' AND NOT EXISTS(SELECT 1 FROM user_sessions s WHERE s.user_id=u.id) AND NOT EXISTS(SELECT 1 FROM organization_memberships other WHERE other.organization_id=m.organization_id AND other.user_id<>u.id) AND NOT EXISTS(SELECT 1 FROM organization_memberships external WHERE external.user_id=u.id AND external.organization_id<>m.organization_id) LIMIT 100), deleted_organizations AS (DELETE FROM organizations o USING candidates c WHERE o.id=c.organization_id RETURNING c.user_id) DELETE FROM users u USING deleted_organizations d WHERE u.id=d.user_id")
        .execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_escape_dynamic_html_and_cover_locales() {
        let data = TemplateData::ApplicationCreated {
            application_name: "<script>\r\n".into(),
            project_name: "A&B".into(),
        };
        for locale in [Locale::En, Locale::Ru] {
            let mail = render(locale, &data);
            assert!(!mail.subject.contains('\n'));
            assert!(!mail.html.contains("<script>"));
            assert!(mail.html.contains("&lt;script&gt;"));
            assert!(mail.html.contains("A&amp;B"));
            assert!(!mail.text.is_empty());
        }
    }

    #[test]
    fn config_rejects_missing_and_insecure_production_settings() {
        let args = MailArgs {
            enabled: false,
            public_web_url: None,
            smtp_host: None,
            smtp_port: 587,
            smtp_tls: "starttls".into(),
            smtp_username: None,
            smtp_password: None,
            from_address: None,
            from_name: "Okoscope".into(),
            default_locale: "en".into(),
            encryption_key: None,
            poll_ms: 1000,
            claim_size: 25,
            concurrency: 4,
            lease_seconds: 60,
            timeout_seconds: 15,
            max_attempts: 8,
            retention_days: 30,
        };
        assert!(args.build(false, true).is_err());
        let mut enabled = args;
        enabled.enabled = true;
        enabled.public_web_url = Some("http://example.com".into());
        assert!(enabled.build(false, true).is_err());
    }

    #[test]
    fn encryption_uses_unique_nonces_and_binds_all_associated_data() {
        let id = Uuid::new_v4();
        let (first, first_nonce) =
            encrypt_payload(&[1; 32], id, "reset_password", "a@example.com", b"secret").unwrap();
        let (second, second_nonce) =
            encrypt_payload(&[1; 32], id, "reset_password", "a@example.com", b"secret").unwrap();
        assert_ne!(first_nonce, second_nonce);
        assert_ne!(first, second);
        let mail = ClaimedMail {
            id,
            template_kind: "reset_password".into(),
            recipient_email: "a@example.com".into(),
            locale: "en".into(),
            payload_ciphertext: first.clone(),
            payload_nonce: first_nonce.to_vec(),
            attempt_count: 1,
            expires_at: None,
        };
        let mut config = MailConfig {
            encryption_key: [1; 32],
            ..MailConfig::default()
        };
        assert_eq!(decrypt(&config, &mail).unwrap().as_slice(), b"secret");
        config.encryption_key = [2; 32];
        assert!(decrypt(&config, &mail).is_err());
        config.encryption_key = [1; 32];
        let mut tampered = mail;
        tampered.payload_ciphertext[0] ^= 1;
        assert!(decrypt(&config, &tampered).is_err());
        tampered.payload_ciphertext = first;
        tampered.recipient_email = "b@example.com".into();
        assert!(decrypt(&config, &tampered).is_err());
    }
}
