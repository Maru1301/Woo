use crate::{
    error::{git_failure, AppError},
    git::{GitOutput, GitRunner},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub name: String,
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RemoteList {
    pub remotes: Vec<RemoteInfo>,
}

#[derive(Debug)]
pub struct RemoteTiming {
    pub git: Duration,
    pub parse: Duration,
    pub total: Duration,
}

// `git config -z --get-regexp` emits key LF value NUL. The final
// `.url`/`.pushurl` suffix disambiguates names that themselves contain dots.
pub fn parse_remote_config(bytes: &[u8]) -> Result<RemoteList, AppError> {
    let mut remotes = BTreeMap::<String, RemoteInfo>::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let (key, value) = record.split_once_byte(b'\n').ok_or_else(malformed)?;
        let key = std::str::from_utf8(key).map_err(|_| malformed())?;
        let value = std::str::from_utf8(value).map_err(|_| malformed())?;
        let key = key.strip_prefix("remote.").ok_or_else(malformed)?;
        let (name, push) = if let Some(name) = key.strip_suffix(".pushurl") {
            (name, true)
        } else if let Some(name) = key.strip_suffix(".url") {
            (name, false)
        } else {
            return Err(malformed());
        };
        if name.is_empty() {
            return Err(malformed());
        }
        let remote = remotes
            .entry(name.to_owned())
            .or_insert_with(|| RemoteInfo {
                name: name.to_owned(),
                fetch_url: None,
                push_url: None,
            });
        // Git supports multiple URLs. M8 displays the first configured URL;
        // Git itself still uses its complete configuration for operations.
        let target = if push {
            &mut remote.push_url
        } else {
            &mut remote.fetch_url
        };
        if target.is_none() {
            *target = Some(redact_url(value));
        }
    }
    Ok(RemoteList {
        remotes: remotes.into_values().collect(),
    })
}

fn malformed() -> AppError {
    AppError::new(
        "invalid_git_output",
        "Git returned malformed remote configuration.",
    )
}

trait SplitOnceByte {
    fn split_once_byte(&self, byte: u8) -> Option<(&[u8], &[u8])>;
}
impl SplitOnceByte for [u8] {
    fn split_once_byte(&self, byte: u8) -> Option<(&[u8], &[u8])> {
        let index = self.iter().position(|item| *item == byte)?;
        Some((&self[..index], &self[index + 1..]))
    }
}

pub fn redact_url(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        if let Some(at) = url.find('@') {
            if let Some(colon) = url[at + 1..].find(':') {
                return format!("[redacted]@{}", &url[at + 1..at + 1 + colon]);
            }
        }
        return url.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority_end = url[authority_start..]
        .find(['/', '?', '#'])
        .map(|i| authority_start + i)
        .unwrap_or(url.len());
    let authority = &url[authority_start..authority_end];
    let masked_authority = if let Some(at) = authority.rfind('@') {
        format!(
            "{}[redacted]@{}",
            &url[..authority_start],
            &url[authority_start + at + 1..authority_end]
        )
    } else {
        url[..authority_end].to_owned()
    };
    // Network URL paths and queries may themselves carry tokens. The remote
    // name is the operation identity; display only a safe authority hint.
    if url[..scheme_end].eq_ignore_ascii_case("file") {
        url.to_owned()
    } else {
        masked_authority
    }
}

pub fn redact_diagnostic(text: &str) -> String {
    // Git may include a credential-bearing URL in its error. Redact any
    // URL authority userinfo, even if it is not the configured remote URL.
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find("://") {
        let start = rest[..index]
            .rfind(|c: char| c.is_whitespace() || c == '\'' || c == '"')
            .map(|i| i + 1)
            .unwrap_or(0);
        out.push_str(&rest[..start]);
        let end = rest[index + 3..]
            .find(|c: char| c.is_whitespace() || c == '\'' || c == '"')
            .map(|i| index + 3 + i)
            .unwrap_or(rest.len());
        out.push_str("[remote URL]");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

pub fn remote_failure(output: &GitOutput) -> AppError {
    let raw = if output.stderr.is_empty() {
        &output.stdout
    } else {
        &output.stderr
    };
    let detail = redact_diagnostic(&String::from_utf8_lossy(raw));
    let code = if detail.contains("index.lock") {
        "repository_locked"
    } else if detail.contains("no upstream branch") {
        "no_upstream"
    } else {
        "remote_failed"
    };
    AppError::new(
        code,
        format!("Git remote operation failed: {}", detail.trim()),
    )
}

pub async fn load_remotes(
    git: &GitRunner,
    repository: &Path,
) -> Result<(RemoteList, RemoteTiming), AppError> {
    if !repository.is_dir() {
        return Err(AppError::new(
            "repository_missing",
            "The open repository directory no longer exists.",
        ));
    }
    let started = Instant::now();
    let output = git
        .run(
            repository,
            &[
                "config",
                "--null",
                "--get-regexp",
                "^remote\\..*\\.(url|pushurl)$",
            ],
        )
        .await
        .map_err(AppError::from)?;
    // `git config --get-regexp` exits 1 for no matching keys.
    if output.exit_code != Some(0)
        && !(output.exit_code == Some(1) && output.stdout.is_empty() && output.stderr.is_empty())
    {
        return Err(git_failure(&output));
    }
    let parsing = Instant::now();
    let list = parse_remote_config(&output.stdout)?;
    let timing = RemoteTiming {
        git: output.duration,
        parse: parsing.elapsed(),
        total: started.elapsed(),
    };
    eprintln!(
        "Git.Remotes git_ms={} parse_us={} total_ms={} count={}",
        timing.git.as_millis(),
        timing.parse.as_micros(),
        timing.total.as_millis(),
        list.remotes.len()
    );
    Ok((list, timing))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_remotes_and_redacts_credentials() {
        let parsed = parse_remote_config(b"remote.origin.url\nhttps://user:secret@example.com/a\0remote.origin.pushurl\nssh://user@host/a\0remote.a.b.url\nfile:///tmp/repo\0").unwrap();
        assert_eq!(parsed.remotes.len(), 2);
        assert_eq!(parsed.remotes[0].name, "a.b");
        assert_eq!(
            parsed.remotes[1].fetch_url.as_deref(),
            Some("https://[redacted]@example.com")
        );
        assert!(
            !redact_diagnostic("fatal: https://user:secret@example.com/a failed")
                .contains("secret")
        );
        assert_eq!(
            redact_url("https://user:secret@example.com/a?token=hidden"),
            "https://[redacted]@example.com"
        );
        assert!(
            !redact_diagnostic("fatal: https://example.com/a?token=hidden failed")
                .contains("hidden")
        );
    }
    #[test]
    fn empty_and_malformed() {
        assert!(parse_remote_config(b"").unwrap().remotes.is_empty());
        assert!(parse_remote_config(b"bad").is_err());
        let unicode =
            parse_remote_config("remote.來源.url\nssh://user@example.com/repo\0".as_bytes())
                .unwrap();
        assert_eq!(unicode.remotes[0].name, "來源");
    }
}
