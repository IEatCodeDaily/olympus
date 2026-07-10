//! Hall-local users, organizations, memberships, and revocable login sessions.
//!
//! This is deliberately separate from the domain event log: password hashes and
//! revocation records are security/operational truth, not replayable business events.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Principal {
    pub user_id: String,
    pub username: String,
    pub kind: String,
}

impl Principal {
    pub const USER_KIND: &'static str = "user";
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub slug: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Clone)]
pub struct CreatedSession {
    pub token: String,
    pub token_hash: String,
    pub expires_at: i64,
}

pub struct AuthStore {
    connection: Mutex<Connection>,
}

impl AuthStore {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating auth store directory {}", parent.display()))?;
            secure_permissions(parent, 0o700)?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("creating auth store {}", path.display()))?;
        secure_permissions(path, 0o600)?;
        let connection = Connection::open(path)
            .with_context(|| format!("opening auth store {}", path.display()))?;
        Self::initialize(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(connection: Connection) -> Result<Self> {
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS users (
               id TEXT PRIMARY KEY,
               username TEXT NOT NULL UNIQUE,
               password_hash TEXT NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS organizations (
               id TEXT PRIMARY KEY,
               slug TEXT NOT NULL UNIQUE,
               display_name TEXT NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS organization_memberships (
               organization_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
               user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
               role TEXT NOT NULL,
               PRIMARY KEY (organization_id, user_id)
             );
             CREATE TABLE IF NOT EXISTS auth_sessions (
               token_hash TEXT PRIMARY KEY,
               user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
               expires_at INTEGER NOT NULL,
               created_at INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS auth_sessions_expiry ON auth_sessions(expires_at);",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn bootstrap_admin(
        &self,
        username: &str,
        password: &str,
        organization_slug: &str,
        organization_name: &str,
    ) -> Result<()> {
        validate_username(username)?;
        validate_password(password)?;
        validate_slug(organization_slug)?;
        let password_hash = hash_password(password)?;
        let now = unix_timestamp();
        let mut connection = self.connection.lock().expect("auth store mutex poisoned");
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing_users: i64 =
            transaction.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        if existing_users > 0 {
            transaction.commit()?;
            return Ok(());
        }

        let user_id = uuid::Uuid::new_v4().to_string();
        let organization_id = uuid::Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO users(id, username, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![user_id, username, password_hash, now],
        )?;
        transaction.execute(
            "INSERT INTO organizations(id, slug, display_name, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![organization_id, organization_slug, organization_name, now],
        )?;
        transaction.execute(
            "INSERT INTO organization_memberships(organization_id, user_id, role) VALUES (?1, ?2, 'owner')",
            params![organization_id, user_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn authenticate(&self, username: &str, password: &str) -> Result<Option<Principal>> {
        let connection = self.connection.lock().expect("auth store mutex poisoned");
        let row: Option<(String, String, String)> = connection
            .query_row(
                "SELECT id, username, password_hash FROM users WHERE username = ?1",
                [username],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let exists = row.is_some();
        let (user_id, username, password_hash) = row.unwrap_or_else(|| {
            (
                String::new(),
                String::new(),
                dummy_password_hash().to_string(),
            )
        });
        let parsed = PasswordHash::new(&password_hash)
            .map_err(|error| anyhow::anyhow!("stored password hash is invalid: {error}"))?;
        let password_valid = Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok();
        if !exists || !password_valid {
            return Ok(None);
        }
        Ok(Some(Principal {
            user_id,
            username,
            kind: Principal::USER_KIND.to_string(),
        }))
    }

    pub fn create_session(
        &self,
        user_id: &str,
        now: i64,
        ttl_seconds: i64,
    ) -> Result<CreatedSession> {
        if ttl_seconds <= 0 {
            bail!("session TTL must be positive");
        }
        let token = random_token();
        let token_hash = hash_token(&token);
        let expires_at = now.saturating_add(ttl_seconds);
        let connection = self.connection.lock().expect("auth store mutex poisoned");
        connection.execute(
            "INSERT INTO auth_sessions(token_hash, user_id, expires_at, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![token_hash, user_id, expires_at, now],
        )?;
        Ok(CreatedSession {
            token,
            token_hash,
            expires_at,
        })
    }

    pub fn resolve_session(&self, token: &str, now: i64) -> Result<Option<Principal>> {
        let token_hash = hash_token(token);
        let connection = self.connection.lock().expect("auth store mutex poisoned");
        connection.execute("DELETE FROM auth_sessions WHERE expires_at < ?1", [now])?;
        connection
            .query_row(
                "SELECT u.id, u.username
                 FROM auth_sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_at >= ?2",
                params![token_hash, now],
                |row| {
                    Ok(Principal {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        kind: Principal::USER_KIND.to_string(),
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn revoke_session(&self, token: &str) -> Result<()> {
        let connection = self.connection.lock().expect("auth store mutex poisoned");
        connection.execute(
            "DELETE FROM auth_sessions WHERE token_hash = ?1",
            [hash_token(token)],
        )?;
        Ok(())
    }

    pub fn create_organization(
        &self,
        slug: &str,
        display_name: &str,
        owner_user_id: &str,
    ) -> Result<Organization> {
        validate_slug(slug)?;
        let display_name = display_name.trim();
        if display_name.is_empty() || display_name.len() > 100 {
            bail!("organization display name must be 1-100 characters");
        }
        let id = uuid::Uuid::new_v4().to_string();
        let mut connection = self.connection.lock().expect("auth store mutex poisoned");
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO organizations(id, slug, display_name, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![id, slug, display_name, unix_timestamp()],
        )?;
        transaction.execute(
            "INSERT INTO organization_memberships(organization_id, user_id, role) VALUES (?1, ?2, 'owner')",
            params![id, owner_user_id],
        )?;
        transaction.commit()?;
        Ok(Organization {
            id,
            slug: slug.to_string(),
            display_name: display_name.to_string(),
            role: "owner".to_string(),
        })
    }

    pub fn organizations_for_user(&self, user_id: &str) -> Result<Vec<Organization>> {
        let connection = self.connection.lock().expect("auth store mutex poisoned");
        let mut statement = connection.prepare(
            "SELECT o.id, o.slug, o.display_name, m.role
             FROM organizations o
             JOIN organization_memberships m ON m.organization_id = o.id
             WHERE m.user_id = ?1 ORDER BY o.display_name COLLATE NOCASE, o.id",
        )?;
        let rows = statement.query_map([user_id], |row| {
            Ok(Organization {
                id: row.get(0)?,
                slug: row.get(1)?,
                display_name: row.get(2)?,
                role: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn user_has_organization(&self, user_id: &str, organization_id: &str) -> Result<bool> {
        let connection = self.connection.lock().expect("auth store mutex poisoned");
        let count: i64 = connection.query_row(
            "SELECT COUNT(*) FROM organization_memberships WHERE user_id = ?1 AND organization_id = ?2",
            params![user_id, organization_id],
            |row| row.get(0),
        )?;
        Ok(count == 1)
    }
}

#[cfg(unix)]
fn secure_permissions(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .with_context(|| format!("setting secure permissions on {}", path.display()))
}

#[cfg(not(unix))]
fn secure_permissions(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn validate_username(username: &str) -> Result<()> {
    if username.len() < 3
        || username.len() > 64
        || !username.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        bail!("username must be 3-64 ASCII letters, numbers, dot, dash, or underscore");
    }
    Ok(())
}

fn validate_password(password: &str) -> Result<()> {
    if password.len() < 8 || password.len() > 1024 {
        bail!("password must be 8-1024 bytes");
    }
    Ok(())
}

fn validate_slug(slug: &str) -> Result<()> {
    if slug.len() < 2
        || slug.len() > 64
        || slug.starts_with('-')
        || slug.ends_with('-')
        || !slug.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        bail!("organization slug must be 2-64 lowercase letters, numbers, or dashes");
    }
    Ok(())
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!("hashing password: {error}"))
}

fn dummy_password_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| hash_password("olympus-dummy-password").expect("hashing dummy password"))
}

fn random_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_token(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn unix_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
