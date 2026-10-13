//! Rows and replies the cloud views read. Port of src/cloud/types.ts. Results of the
//! Rust backend commands (`Preflight`, `LocalInstallation`, `CloudRuntimeInfo`, ...) keep
//! their backend types.

/// The signed-in cloud user.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub access_token: String,
    pub user_id: String,
    pub email: Option<String>,
}

impl Session {
    /// The email, or the user id when the account has none.
    pub fn display(&self) -> String {
        self.email.clone().unwrap_or_else(|| self.user_id.clone())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Organization {
    pub id: String,
    pub name: String,
    /// `owner`, `admin`, `billing` or `member`.
    pub role: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CloudApp {
    pub id: String,
    pub org_id: String,
    pub owner_id: String,
    pub name: String,
    pub document_id: String,
    pub backups_enabled: bool,
    pub head_version_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppVersion {
    pub id: String,
    pub version: String,
    pub created_at: String,
    pub archive_sha256: String,
    pub archive_size: u64,
    pub release_notes: String,
    /// `pending`, `published` or `withdrawn`.
    pub status: String,
    pub min_runtime_version: String,
    pub resolution: Option<String>,
}

/// An app the signed-in user may run (active membership).
#[derive(Debug, Clone, PartialEq)]
pub struct MemberApp {
    pub app_id: String,
    pub name: String,
    pub role_id: String,
    pub role_name: String,
}

/// A runtime installation backup (`installation_backups` row).
#[derive(Debug, Clone, PartialEq)]
pub struct InstallationBackup {
    pub id: String,
    pub installation_id: String,
    pub archive_sha256: String,
    pub archive_size: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestoreTarget {
    pub signed_url: String,
    pub sha256: String,
    pub size: u64,
    pub is_postgres: bool,
    pub warning: Option<String>,
}

/// A version row as returned by publish and resolve.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishedVersion {
    pub id: String,
    pub version: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AppRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LatestVersion {
    pub version_id: String,
    pub version: String,
    pub min_runtime_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncCheck {
    pub up_to_date: bool,
    pub latest: Option<LatestVersion>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Membership {
    /// `app` or `org`.
    pub kind: String,
    pub app_id: Option<String>,
    pub org_id: String,
}

/// Tokens of a session handed over by the browser sign-in.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionTokens {
    pub access_token: String,
    pub refresh_token: String,
}
