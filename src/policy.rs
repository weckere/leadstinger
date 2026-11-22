//! What a caller may do: the rules behind `leadstinger-cli --safe` and `--allow-dir`.
//!
//! The CLI checks them before it sends a command, and the control socket checks them
//! again for every command on a connection that asked for them (the `policy` message),
//! so a script or client talking to the socket directly gets the same refusals. A
//! connection can only narrow its policy, never widen it again.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::commands::Spec;
use crate::controller::CmdError;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Policy {
    /// Refuse destructive commands, quitting the app, trusting QuipClipper servers, and
    /// local files outside `allow_dirs`.
    pub safe: bool,
    /// Under `safe`, the folders whose files may be named (imports, pictures, exports).
    pub allow_dirs: Vec<PathBuf>,
}

impl Policy {
    /// The unrestricted policy of the GUI and of an unrestricted connection.
    pub fn open() -> Policy {
        Policy::default()
    }

    pub fn safe(allow_dirs: Vec<PathBuf>) -> Policy {
        Policy { safe: true, allow_dirs }
    }

    /// Refuse a command this policy doesn't allow (code `refused`).
    pub fn check(&self, spec: &Spec, params: &Value) -> Result<(), CmdError> {
        if !self.safe {
            return Ok(());
        }
        let refuse = |m: String| Err(CmdError { code: "refused", message: m, candidates: vec![] });
        if spec.destructive {
            return refuse(format!("{} is destructive; --safe refuses it", spec.id));
        }
        if spec.id == "app.quit" {
            return refuse("--safe doesn't quit the app; ask whoever is at it".into());
        }
        if matches!(spec.id, "quip.connect" | "quip.disconnect" | "quip.pair") {
            return refuse("--safe doesn't change which QuipClipper servers are trusted; ask the user".into());
        }
        let mut paths: Vec<&str> = Vec::new();
        for &key in local_paths(spec) {
            match params.get(key) {
                Some(Value::String(s)) => paths.push(s),
                Some(Value::Array(a)) => paths.extend(a.iter().filter_map(Value::as_str)),
                _ => {}
            }
        }
        for p in paths {
            if !self.allows_path(Path::new(p)) {
                return refuse(format!("--safe: {p} isn't inside an --allow-dir folder"));
            }
        }
        Ok(())
    }

    fn allows_path(&self, p: &Path) -> bool {
        let real = real_path(p);
        self.allow_dirs.iter().any(|d| real.starts_with(d))
    }

    /// This policy narrowed by `other`: safe once either is, and under safe only the
    /// allowed folders that lie inside one already allowed.
    pub fn narrow(&self, other: &Policy) -> Policy {
        match (self.safe, other.safe) {
            (false, _) => other.clone(),
            (true, false) => self.clone(),
            (true, true) => Policy {
                safe: true,
                allow_dirs: other.allow_dirs.iter().filter(|d| self.allows_path(d)).cloned().collect(),
            },
        }
    }

    /// Read `{safe?, allowDirs?}` (the socket's `policy` message).
    pub fn from_json(v: &Value) -> Result<Policy, CmdError> {
        let bad = |m: &str| CmdError { code: "bad_params", message: format!("policy: {m}"), candidates: vec![] };
        let safe = match v.get("safe") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(bad("safe must be true or false")),
        };
        let allow_dirs = match v.get("allowDirs") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => a
                .iter()
                .map(|d| d.as_str().map(|s| real_path(Path::new(s))))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| bad("allowDirs must be a list of folders"))?,
            Some(_) => return Err(bad("allowDirs must be a list of folders")),
        };
        Ok(Policy { safe, allow_dirs })
    }

    pub fn to_json(&self) -> Value {
        json!({"safe": self.safe, "allowDirs": self.allow_dirs})
    }
}

/// A path with its links resolved, as far up as it exists: a file still to be written (an
/// export) sits inside an allowed folder even where that folder's path runs through a link
/// (macOS's /var is /private/var).
fn real_path(p: &Path) -> PathBuf {
    if let Ok(real) = p.canonicalize() {
        return real;
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => real_path(parent).join(name),
        _ => p.to_path_buf(),
    }
}

/// The parameters that name files or folders on this machine. A QuipClipper command's
/// `folder` (and the QuipClipper window's, `view.quip`) is a folder in that server's
/// clips library, sent as it is.
pub fn local_paths(spec: &Spec) -> &'static [&'static str] {
    if spec.id.starts_with("quip.") || spec.id == "view.quip" {
        &["path", "paths"]
    } else if spec.id == "settings.transcribe" {
        // A model file is read by whisper-cli at each transcription.
        &["whisperModel"]
    } else {
        &["path", "paths", "folder"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::spec;

    #[test]
    fn safe_refuses_what_the_cli_refused() {
        let p = Policy::safe(vec![]);
        for id in ["tile.remove", "set.delete", "app.quit", "quip.connect", "quip.pair"] {
            assert_eq!(p.check(spec(id).unwrap(), &json!({})).unwrap_err().code, "refused", "{id}");
        }
        assert!(p.check(spec("tile.trigger").unwrap(), &json!({"tile": "x"})).is_ok());
        assert!(Policy::open().check(spec("tile.remove").unwrap(), &json!({})).is_ok());
    }

    #[test]
    fn safe_files_must_be_in_an_allowed_folder() {
        let root = std::env::temp_dir().join(format!("sb-policy-{}", uuid::Uuid::new_v4()));
        let inside = root.join("drop");
        std::fs::create_dir_all(&inside).unwrap();
        let p = Policy::safe(vec![inside.canonicalize().unwrap()]);
        let add = spec("tile.add").unwrap();
        assert!(p.check(add, &json!({"paths": [inside.join("a.wav")]})).is_ok());
        assert_eq!(p.check(add, &json!({"paths": [root.join("b.wav")]})).unwrap_err().code, "refused");
        // A QuipClipper folder is on the server, not here.
        assert!(p.check(spec("quip.browse").unwrap(), &json!({"folder": "/anything"})).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_policy_only_narrows() {
        let root = std::env::temp_dir().join(format!("sb-narrow-{}", uuid::Uuid::new_v4()));
        let (a, b) = (root.join("a"), root.join("b"));
        std::fs::create_dir_all(a.join("inner")).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let a = a.canonicalize().unwrap();
        let safe_a = Policy::open().narrow(&Policy::safe(vec![a.clone()]));
        assert_eq!(safe_a, Policy::safe(vec![a.clone()]));
        // Asking for less is fine; asking for "not safe" or another folder isn't granted.
        assert_eq!(safe_a.narrow(&Policy::open()), safe_a);
        let widened = safe_a.narrow(&Policy::safe(vec![b.canonicalize().unwrap(), a.join("inner")]));
        assert_eq!(widened, Policy::safe(vec![a.join("inner")]));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn policies_read_from_json() {
        assert_eq!(Policy::from_json(&json!({})).unwrap(), Policy::open());
        assert!(Policy::from_json(&json!({"safe": true})).unwrap().safe);
        assert_eq!(Policy::from_json(&json!({"safe": "yes"})).unwrap_err().code, "bad_params");
        assert_eq!(Policy::from_json(&json!({"allowDirs": "/tmp"})).unwrap_err().code, "bad_params");
    }
}
