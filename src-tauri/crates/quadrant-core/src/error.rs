//! Stable error codes for failures a host shows to the user.
//!
//! A code's [`ErrorCode::key`] is an i18n key the frontend translates, so the
//! wording of a Rust error never decides what the user reads.

use std::fmt;

/// A failure the user can act on. `Display` prints the i18n key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    ThirdPartyDownloadDisabled,
    DownloadRefused,
    InvalidRequest,
    Timeout,
    RateLimited,
    Forbidden,
    NotFound,
    Server,
    Network,
    BadResponse,
    Checksum,
    UnsafeDownload,
    CurseforgeDisabled,
    ModpackMissing,
    ModpackExists,
    InvalidModpackName,
    ModAlreadyRegistered,
    ContentMissing,
    FileAccess,
    FileInUse,
    DiskFull,
    NoMinecraftFolder,
    SignedOut,
    Busy,
    PrismInstanceMissing,
    PrismInstanceUnreadable,
    PrismLoaderVersionRequired,
    PrismLoaderVersionNotFound,
    CloudSyncNewer,
}

impl ErrorCode {
    pub const ALL: [ErrorCode; 29] = [
        Self::ThirdPartyDownloadDisabled,
        Self::DownloadRefused,
        Self::InvalidRequest,
        Self::Timeout,
        Self::RateLimited,
        Self::Forbidden,
        Self::NotFound,
        Self::Server,
        Self::Network,
        Self::BadResponse,
        Self::Checksum,
        Self::UnsafeDownload,
        Self::CurseforgeDisabled,
        Self::ModpackMissing,
        Self::ModpackExists,
        Self::InvalidModpackName,
        Self::ModAlreadyRegistered,
        Self::ContentMissing,
        Self::FileAccess,
        Self::FileInUse,
        Self::DiskFull,
        Self::NoMinecraftFolder,
        Self::SignedOut,
        Self::Busy,
        Self::PrismInstanceMissing,
        Self::PrismInstanceUnreadable,
        Self::PrismLoaderVersionRequired,
        Self::PrismLoaderVersionNotFound,
        Self::CloudSyncNewer,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::ThirdPartyDownloadDisabled => "thirdPartyDownloadDisabled",
            Self::DownloadRefused => "errorDownloadRefused",
            Self::InvalidRequest => "errorInvalidRequest",
            Self::Timeout => "errorTimeout",
            Self::RateLimited => "errorRateLimited",
            Self::Forbidden => "errorForbidden",
            Self::NotFound => "errorNotFound",
            Self::Server => "errorServer",
            Self::Network => "errorNetwork",
            Self::BadResponse => "errorBadResponse",
            Self::Checksum => "errorChecksum",
            Self::UnsafeDownload => "errorUnsafeDownload",
            Self::CurseforgeDisabled => "errorCurseforgeDisabled",
            Self::ModpackMissing => "errorModpackMissing",
            Self::ModpackExists => "errorModpackExists",
            Self::InvalidModpackName => "errorInvalidModpackName",
            Self::ModAlreadyRegistered => "errorModAlreadyRegistered",
            Self::ContentMissing => "errorContentMissing",
            Self::FileAccess => "errorFileAccess",
            Self::FileInUse => "errorFileInUse",
            Self::DiskFull => "errorDiskFull",
            Self::NoMinecraftFolder => "errorNoMinecraftFolder",
            Self::SignedOut => "errorSignedOut",
            Self::Busy => "errorBusy",
            Self::PrismInstanceMissing => "errorPrismInstanceMissing",
            Self::PrismInstanceUnreadable => "errorPrismInstanceUnreadable",
            Self::PrismLoaderVersionRequired => "errorPrismLoaderVersionRequired",
            Self::PrismLoaderVersionNotFound => "errorPrismLoaderVersionNotFound",
            Self::CloudSyncNewer => "errorCloudSyncNewer",
        }
    }

    /// The code for an unsuccessful HTTP status, if it has one.
    pub fn from_status(status: reqwest::StatusCode) -> Option<Self> {
        match status.as_u16() {
            429 => Some(Self::RateLimited),
            401 | 403 => Some(Self::Forbidden),
            404 => Some(Self::NotFound),
            500..=599 => Some(Self::Server),
            _ => None,
        }
    }

    fn from_reqwest(error: &reqwest::Error) -> Option<Self> {
        if error.is_builder() {
            Some(Self::InvalidRequest)
        } else if error.is_timeout() {
            Some(Self::Timeout)
        } else if let Some(status) = error.status() {
            // The server answered, so an unmapped status is not a network
            // failure; leave it readable instead.
            Self::from_status(status)
        } else if error.is_decode() {
            Some(Self::BadResponse)
        } else {
            Some(Self::Network)
        }
    }

    fn from_io(error: &std::io::Error) -> Option<Self> {
        // Raw OS codes mean different things per platform, so only Windows'
        // sharing violation (no `ErrorKind` of its own) is read from one.
        #[cfg(windows)]
        if error.raw_os_error() == Some(32) {
            return Some(Self::FileInUse);
        }
        match error.kind() {
            std::io::ErrorKind::StorageFull => Some(Self::DiskFull),
            std::io::ErrorKind::PermissionDenied => Some(Self::FileAccess),
            std::io::ErrorKind::TimedOut => Some(Self::Timeout),
            _ => None,
        }
    }

    fn classify(error: &anyhow::Error) -> Option<Self> {
        // A code attached with `.context(code)` is only reachable through
        // anyhow's own downcast, not through the `chain()` of causes.
        if let Some(code) = error.downcast_ref::<Self>() {
            return Some(*code);
        }
        error.chain().find_map(|cause| {
            if let Some(code) = cause.downcast_ref::<Self>() {
                Some(*code)
            } else if let Some(error) = cause.downcast_ref::<reqwest::Error>() {
                Self::from_reqwest(error)
            } else {
                cause
                    .downcast_ref::<std::io::Error>()
                    .and_then(Self::from_io)
            }
        })
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

impl std::error::Error for ErrorCode {}

/// Whether an error is the cloud-sync conflict the backend reports when a
/// submitted modpack is older than the copy it would overwrite.
///
/// Callers branch on this instead of comparing the backend's wording.
pub fn is_cloud_sync_conflict(error: &anyhow::Error) -> bool {
    ErrorCode::classify(error) == Some(ErrorCode::CloudSyncNewer)
}

/// Whether an error means there is no usable session, for example because the
/// refresh token was refused and the credentials were cleared.
pub fn is_signed_out(error: &anyhow::Error) -> bool {
    ErrorCode::classify(error) == Some(ErrorCode::SignedOut)
}

/// Converts an error for display by a host: a recognised failure becomes its
/// [`ErrorCode`] key, anything else passes through unchanged.
///
/// Hosts call this once, where errors leave the backend.
pub fn user_facing(error: anyhow::Error) -> anyhow::Error {
    match ErrorCode::classify(&error) {
        Some(code) => {
            log::warn!("{}: {error:#}", code.key());
            code.into()
        }
        None => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Context, anyhow};

    #[test]
    fn every_code_has_a_translation_in_every_locale() {
        let locales = [
            ("en", include_str!("../../../../src/locales/en.json")),
            ("tr", include_str!("../../../../src/locales/tr.json")),
            ("uk", include_str!("../../../../src/locales/uk.json")),
        ];
        for (name, json) in locales {
            let strings: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(json).unwrap();
            for code in ErrorCode::ALL {
                assert!(
                    strings.contains_key(code.key()),
                    "{name}.json is missing {}",
                    code.key()
                );
            }
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<_> = ErrorCode::ALL.iter().map(|code| code.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), ErrorCode::ALL.len());
    }

    #[test]
    fn a_code_added_as_context_wins_over_the_error_it_wraps() {
        let io_error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let error = anyhow::Error::from(io_error).context(ErrorCode::DownloadRefused);
        assert_eq!(user_facing(error).to_string(), "errorDownloadRefused");
    }

    #[test]
    fn a_code_survives_added_context() {
        let error = anyhow::Error::from(ErrorCode::ModpackMissing).context("while applying");
        assert_eq!(user_facing(error).to_string(), "errorModpackMissing");
    }

    #[test]
    fn io_errors_are_classified_by_kind() {
        let cases = [
            (
                std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                Some("errorFileAccess"),
            ),
            (
                std::io::Error::from(std::io::ErrorKind::StorageFull),
                Some("errorDiskFull"),
            ),
            (std::io::Error::from(std::io::ErrorKind::NotFound), None),
            // EPIPE on Linux, a sharing violation on Windows.
            #[cfg(not(windows))]
            (std::io::Error::from_raw_os_error(32), None),
            #[cfg(windows)]
            (
                std::io::Error::from_raw_os_error(32),
                Some("errorFileInUse"),
            ),
        ];
        for (io_error, expected) in cases {
            let raw = io_error.to_string();
            let shown = user_facing(anyhow::Error::from(io_error)).to_string();
            assert_eq!(shown, expected.unwrap_or(&raw));
        }
    }

    #[test]
    fn http_statuses_map_to_codes() {
        let cases = [
            (429, Some(ErrorCode::RateLimited)),
            (401, Some(ErrorCode::Forbidden)),
            (403, Some(ErrorCode::Forbidden)),
            (404, Some(ErrorCode::NotFound)),
            (503, Some(ErrorCode::Server)),
            (400, None),
        ];
        for (status, expected) in cases {
            let status = reqwest::StatusCode::from_u16(status).unwrap();
            assert_eq!(ErrorCode::from_status(status), expected);
        }
    }

    #[test]
    fn a_request_without_a_valid_url_is_an_invalid_request() {
        let error = reqwest::Client::new().get("").build().unwrap_err();
        assert_eq!(
            user_facing(anyhow::Error::from(error)).to_string(),
            "errorInvalidRequest"
        );
    }

    #[test]
    fn unrecognised_errors_pass_through() {
        let error = Err::<(), _>(anyhow!("noVersion"))
            .context("checking updates")
            .unwrap_err();
        assert_eq!(user_facing(error).to_string(), "checking updates");
    }
}
