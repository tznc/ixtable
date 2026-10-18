//! Plain-language explanations of bundle errors. Port of src/release/errors.ts and
//! BundleError.tsx. The banner is the shared error callout with a bundle-specific headline.

use crate::{manager::AppError, ui::prelude::*};

fn summary(code: &str) -> &'static str {
    match code {
        "BUNDLE_SIGNATURE" => "This file is not a valid ixtable runtime bundle, or it was changed after it was signed. It was not opened.",
        "BUNDLE_PASSWORD" => "The password is incorrect.",
        "BUNDLE_PASSWORD_REQUIRED" => "This bundle is password protected. Enter its password.",
        "BUNDLE_INCOMPATIBLE" => "This bundle needs a newer version of ixtable.",
        "BUNDLE_SIGNER_MISMATCH" => "This file is signed by a different developer key than the installed application. It was not opened.",
        "BUNDLE_DOWNGRADE" => "A newer version of this application is already installed.",
        "BUNDLE_SAME_VERSION" => "This version is already installed.",
        "BUNDLE_MISMATCH" => "This file is a different application.",
        "BUNDLE_NOT_INSTALLED" => "Open this bundle from the start screen before updating it.",
        "UPDATE_FAILED" => "The update failed and was rolled back. The previous version is still in use.",
        "INSTALL_FAILED" => "The bundle could not be installed. Nothing was changed.",
        "INVALID_VERSION" => "Use a semantic version such as 1.2.0.",
        "VALIDATION_FAILED" => "Fix the problems in this application before exporting.",
        "EXPRESSION_ERRORS" => "Some expressions have errors. Fix them in Settings › Problems before exporting.",
        "READ_ONLY" => "Runtime bundles are read-only.",
        "MISSING_FILE" => "The file could not be read.",
        _ => "The operation failed.",
    }
}

/// Plain-language explanation for a bundle error code, plus the backend detail.
pub fn bundle_error_text(error: &AppError) -> (String, String) {
    (summary(&error.code).to_string(), error.message.clone())
}

pub fn is_password_error(error: &AppError) -> bool {
    matches!(error.code.as_str(), "BUNDLE_PASSWORD" | "BUNDLE_PASSWORD_REQUIRED")
}

/// The error callout for a bundle error: the plain-language summary as its headline
/// (`<id>-headline`), the backend detail under `id`, and the code (`<id>-code`).
pub fn bundle_error_banner(id: &'static str, error: &AppError, cx: &App) -> Div {
    widgets::error_callout(id, summary(&error.code), error, None, cx)
}

#[cfg(test)]
mod tests {
    use super::{bundle_error_text, is_password_error};
    use crate::manager::AppError;

    #[test]
    fn explains_known_codes_and_keeps_the_backend_detail() {
        let (summary, detail) = bundle_error_text(&AppError::new("BUNDLE_PASSWORD", "bad tag"));
        assert_eq!(summary, "The password is incorrect.");
        assert_eq!(detail, "bad tag");
        let (summary, _) = bundle_error_text(&AppError::new("SOMETHING_ELSE", "x"));
        assert_eq!(summary, "The operation failed.");
    }

    #[test]
    fn recognizes_password_errors() {
        assert!(is_password_error(&AppError::new("BUNDLE_PASSWORD_REQUIRED", "")));
        assert!(!is_password_error(&AppError::new("BUNDLE_SIGNATURE", "")));
    }
}
