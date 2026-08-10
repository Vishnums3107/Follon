//! Secret-provider boundary for deployment adapters and hosted infrastructure.
//!
//! The managed-command provider invokes one fixed, absolute executable without
//! a shell and receives bounded secret bytes over a private pipe. This supports
//! audited vault/keychain helpers while keeping environment variables, files,
//! strategy code, configuration, and logs free of credential material.

use std::fmt;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Opaque, validated reference to a secret managed outside the trading core.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SecretReference(String);

impl SecretReference {
    /// Creates a reference such as `secret.broker.ibkr.account-001`.
    pub fn new(value: impl Into<String>) -> Result<Self, SecretError> {
        let value = value.into();
        if value.is_empty()
            || !value.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'_' | b'-')
            })
        {
            return Err(SecretError(
                "secret reference must be a canonical ID".to_owned(),
            ));
        }
        Ok(Self(value))
    }

    /// Returns the non-sensitive reference used for audit and access policy.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Secret bytes intentionally omit `Debug`, `Clone`, and string conversion.
pub struct SecretMaterial(Vec<u8>);

impl SecretMaterial {
    /// Constructs secret material at a trusted provider boundary.
    pub fn new(bytes: Vec<u8>) -> Result<Self, SecretError> {
        if bytes.is_empty() {
            return Err(SecretError("secret material cannot be empty".to_owned()));
        }
        Ok(Self(bytes))
    }

    /// Provides bytes only to the adapter operation that requires them.
    pub fn expose_to<T>(&self, operation: impl FnOnce(&[u8]) -> T) -> T {
        operation(&self.0)
    }
}

impl Drop for SecretMaterial {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// Provider errors must be safe to log and must never embed secret material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecretError(pub String);

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SecretError {}

/// Deployment-edge interface for managed vaults and operating-system keychains.
pub trait SecretProvider: Send + Sync {
    /// Retrieves a secret without exposing it to strategy code or logs.
    fn resolve(&self, reference: &SecretReference) -> Result<SecretMaterial, SecretError>;
}

/// Deployment configuration for a fixed vault/keychain helper executable.
pub struct ManagedCommandSecretProvider {
    executable: PathBuf,
    arguments: Vec<String>,
    timeout: Duration,
    max_secret_bytes: usize,
}

impl ManagedCommandSecretProvider {
    /// Creates a provider which appends the canonical secret reference as the final argument.
    ///
    /// `executable` must be an existing absolute file. No shell is used, stderr is discarded,
    /// stdin is closed, output is bounded, and the process is killed on timeout. Fixed arguments
    /// must contain only non-sensitive provider options; secret material must be emitted only on
    /// stdout by the trusted helper.
    pub fn new(
        executable: PathBuf,
        arguments: Vec<String>,
        timeout: Duration,
        max_secret_bytes: usize,
    ) -> Result<Self, SecretError> {
        if !executable.is_absolute() || !executable.is_file() {
            return Err(SecretError(
                "managed secret helper must be an existing absolute executable file".to_owned(),
            ));
        }
        let executable = executable.canonicalize().map_err(|_| {
            SecretError("managed secret helper path could not be resolved".to_owned())
        })?;
        if arguments.len() > 64
            || arguments
                .iter()
                .any(|argument| argument.len() > 4_096 || argument.contains('\0'))
            || timeout < Duration::from_millis(10)
            || timeout > Duration::from_secs(60)
            || !(1..=65_536).contains(&max_secret_bytes)
        {
            return Err(SecretError(
                "managed secret helper limits or arguments are invalid".to_owned(),
            ));
        }
        Ok(Self {
            executable,
            arguments,
            timeout,
            max_secret_bytes,
        })
    }

    fn collect_bounded_stdout(
        stdout: impl Read + Send + 'static,
        max_secret_bytes: usize,
    ) -> thread::JoinHandle<Result<(Vec<u8>, bool), SecretError>> {
        thread::spawn(move || {
            let mut stdout = stdout;
            let mut collected = Vec::with_capacity(max_secret_bytes.min(8_192));
            let mut overflowed = false;
            let mut buffer = [0_u8; 8_192];
            loop {
                let count = stdout.read(&mut buffer).map_err(|_| {
                    SecretError("managed secret helper output could not be read".to_owned())
                })?;
                if count == 0 {
                    break;
                }
                let remaining = max_secret_bytes.saturating_sub(collected.len());
                let retained = count.min(remaining);
                collected.extend_from_slice(&buffer[..retained]);
                overflowed |= retained < count;
            }
            Ok((collected, overflowed))
        })
    }
}

impl SecretProvider for ManagedCommandSecretProvider {
    fn resolve(&self, reference: &SecretReference) -> Result<SecretMaterial, SecretError> {
        let mut child = Command::new(&self.executable)
            .args(&self.arguments)
            .arg(reference.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| SecretError("managed secret helper could not be started".to_owned()))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            SecretError("managed secret helper output pipe is unavailable".to_owned())
        })?;
        let reader = Self::collect_bounded_stdout(stdout, self.max_secret_bytes);
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(SecretError("managed secret helper timed out".to_owned()));
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader.join();
                    return Err(SecretError(
                        "managed secret helper status is unavailable".to_owned(),
                    ));
                }
            }
        };
        let (mut bytes, overflowed) = reader
            .join()
            .map_err(|_| SecretError("managed secret helper reader failed".to_owned()))??;
        if !status.success() {
            bytes.fill(0);
            return Err(SecretError(
                "managed secret helper returned a failure status".to_owned(),
            ));
        }
        if overflowed {
            bytes.fill(0);
            return Err(SecretError(
                "managed secret helper output exceeded its configured limit".to_owned(),
            ));
        }
        if bytes.ends_with(b"\r\n") {
            bytes.truncate(bytes.len() - 2);
        } else if bytes.ends_with(b"\n") {
            bytes.truncate(bytes.len() - 1);
        }
        SecretMaterial::new(bytes)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::{ManagedCommandSecretProvider, SecretProvider, SecretReference};

    #[test]
    fn secret_reference_never_contains_display_whitespace() {
        assert!(SecretReference::new("secret.broker.ibkr.account-001").is_ok());
        assert!(SecretReference::new("IBKR token").is_err());
    }

    #[test]
    fn managed_command_provider_returns_bounded_pipe_material_without_logging_it() {
        let (executable, arguments) = helper_command(
            "[Console]::Out.Write('vault-secret')",
            "printf vault-secret",
        );
        let provider =
            ManagedCommandSecretProvider::new(executable, arguments, Duration::from_secs(5), 64)
                .expect("test helper configuration");
        let reference =
            SecretReference::new("secret.broker.ibkr.account-001").expect("test secret reference");
        let secret = provider
            .resolve(&reference)
            .expect("test secret resolution");
        assert_eq!(secret.expose_to(|bytes| bytes.to_vec()), b"vault-secret");
    }

    #[test]
    fn managed_command_provider_exposes_no_failed_helper_output() {
        let (executable, arguments) = helper_command(
            "[Console]::Out.Write('sensitive'); exit 7",
            "printf sensitive; exit 7",
        );
        let provider =
            ManagedCommandSecretProvider::new(executable, arguments, Duration::from_secs(5), 64)
                .expect("test helper configuration");
        let error = match provider
            .resolve(&SecretReference::new("secret.failed").expect("test reference"))
        {
            Ok(_) => panic!("failed helper must not return material"),
            Err(error) => error,
        };
        assert!(!error.0.contains("sensitive"));
    }

    #[test]
    fn managed_command_provider_rejects_excess_output() {
        let (executable, arguments) =
            helper_command("[Console]::Out.Write('too-long')", "printf too-long");
        let provider =
            ManagedCommandSecretProvider::new(executable, arguments, Duration::from_secs(5), 4)
                .expect("test helper configuration");
        let error = match provider
            .resolve(&SecretReference::new("secret.overflow").expect("test reference"))
        {
            Ok(_) => panic!("oversized helper output must not return material"),
            Err(error) => error,
        };
        assert!(error.0.contains("exceeded"));
        assert!(!error.0.contains("too-long"));
    }

    #[test]
    fn managed_command_provider_kills_a_timed_out_helper() {
        let (executable, arguments) = helper_command(
            "Start-Sleep -Seconds 2; [Console]::Out.Write('late-secret')",
            "sleep 2; printf late-secret",
        );
        let provider =
            ManagedCommandSecretProvider::new(executable, arguments, Duration::from_millis(50), 64)
                .expect("test helper configuration");
        let error = match provider
            .resolve(&SecretReference::new("secret.timeout").expect("test reference"))
        {
            Ok(_) => panic!("timed-out helper must not return material"),
            Err(error) => error,
        };
        assert!(error.0.contains("timed out"));
        assert!(!error.0.contains("late-secret"));
    }

    #[cfg(windows)]
    fn helper_command(power_shell: &str, _shell: &str) -> (PathBuf, Vec<String>) {
        let system_root = std::env::var_os("SystemRoot").expect("Windows system root");
        (
            PathBuf::from(system_root)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe"),
            vec![
                "-NoProfile".to_owned(),
                "-NonInteractive".to_owned(),
                "-Command".to_owned(),
                format!("& {{ param($reference) {power_shell} }}"),
            ],
        )
    }

    #[cfg(not(windows))]
    fn helper_command(_power_shell: &str, shell: &str) -> (PathBuf, Vec<String>) {
        (
            PathBuf::from("/bin/sh"),
            vec![
                "-c".to_owned(),
                shell.to_owned(),
                "follon-secret-helper".to_owned(),
            ],
        )
    }
}
