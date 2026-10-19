//! The headless tool table (layer L4). Every user-facing feature is a tool here, so agents,
//! scripts and tests can drive ScribeCraft without the GUI (`AGENTS.md` §3).
//!
//! Every path argument is resolved inside the `--root` directory and refused otherwise,
//! including through symbolic links. Paths in results are relative to the root.

mod args;
mod tools;

use std::path::{Component, Path, PathBuf};

use scribecraft_engine::{Session, Settings};
use serde_json::Value;

pub use tools::{TOOLS, Tool};

/// A tool failure, with enough detail for an agent to correct the call.
#[derive(Debug, thiserror::Error, PartialEq)]
#[error("{tool}: {message}")]
pub struct ToolError {
    pub tool: String,
    pub message: String,
}

impl ToolError {
    pub(crate) fn new(tool: &str, message: impl Into<String>) -> Self {
        Self { tool: tool.to_owned(), message: message.into() }
    }
}

/// Runs tools against one session, confined to one root directory.
#[derive(Debug)]
pub struct Runner {
    root: PathBuf,
    session: Session,
}

impl Runner {
    /// Creates a runner whose paths are confined to `root` (which must exist).
    pub fn new(root: &Path) -> Result<Self, ToolError> {
        let root = std::fs::canonicalize(root)
            .map_err(|e| ToolError::new("runner", format!("the root directory {} cannot be used: {e}", root.display())))?;
        let session = Session::new(Settings::default()).map_err(|e| ToolError::new("runner", e.to_string()))?;
        Ok(Self { root, session })
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Calls one tool by name.
    pub fn call(&mut self, name: &str, args: &Value) -> Result<Value, ToolError> {
        let Some(tool) = TOOLS.iter().find(|t| t.name == name) else {
            let names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
            return Err(ToolError::new(name, format!("unknown tool; available tools: {}", names.join(", "))));
        };
        let empty = Value::Object(Default::default());
        let args = if args.is_null() { &empty } else { args };
        if !args.is_object() {
            return Err(ToolError::new(name, "arguments must be a JSON object"));
        }
        (tool.run)(self, args).map_err(|message| ToolError::new(name, self.redact(&message)))
    }

    /// Replaces the absolute root in messages, so errors never reveal where it lives.
    fn redact(&self, message: &str) -> String {
        let root = self.root.to_string_lossy();
        // Windows canonical paths carry the verbatim prefix; messages may use either form.
        let plain = root.strip_prefix(r"\\?\").unwrap_or(&root);
        message.replace(&*root, "<root>").replace(plain, "<root>")
    }

    /// Like [`Runner::resolve`], for a file about to be written: creates its missing
    /// parent folders (all inside the root, since the path was confined first).
    pub(crate) fn resolve_output(&self, raw: &str) -> Result<PathBuf, String> {
        let path = self.resolve(raw)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create the folder for {raw:?}: {e}"))?;
        }
        Ok(path)
    }

    /// Runs a script `{"steps": [{"tool": "...", "args": {...}}, ...]}`. Stops at the first
    /// failing step; the error names the step (1-based).
    pub fn run_script(&mut self, script: &Value) -> Result<Vec<Value>, ToolError> {
        let steps =
            script.get("steps").and_then(Value::as_array).ok_or_else(|| ToolError::new("script", "expected an object with a \"steps\" array"))?;
        let mut results = Vec::with_capacity(steps.len());
        for (i, step) in steps.iter().enumerate() {
            let name =
                step.get("tool").and_then(Value::as_str).ok_or_else(|| ToolError::new("script", format!("step {}: missing \"tool\" name", i + 1)))?;
            let args = step.get("args").unwrap_or(&Value::Null);
            let out = self.call(name, args).map_err(|e| ToolError { tool: e.tool, message: format!("step {}: {}", i + 1, e.message) })?;
            results.push(serde_json::json!({ "tool": name, "result": out }));
        }
        Ok(results)
    }

    /// Resolves a user-supplied path inside the root. Refuses `..`, absolute paths outside
    /// the root, and anything whose real location (after symbolic links) is outside it.
    pub(crate) fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let given = Path::new(raw);
        if raw.is_empty() {
            return Err("the path is empty".into());
        }
        if given.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(format!("{raw:?} contains \"..\"; paths must stay inside the root directory"));
        }
        let joined = if given.is_absolute() { given.to_path_buf() } else { self.root.join(given) };
        // The deepest existing ancestor decides where the path really is.
        let mut existing = joined.as_path();
        let mut rest = Vec::new();
        let real = loop {
            match std::fs::canonicalize(existing) {
                Ok(real) => break real,
                Err(_) => match (existing.parent(), existing.file_name()) {
                    (Some(parent), Some(name)) => {
                        rest.push(name.to_os_string());
                        existing = parent;
                    }
                    _ => return Err(format!("{raw:?} is outside the root directory")),
                },
            }
        };
        if !real.starts_with(&self.root) {
            return Err(format!("{raw:?} is outside the root directory; only paths inside it are allowed"));
        }
        Ok(rest.iter().rev().fold(real, |p, n| p.join(n)))
    }

    /// The root-relative form of `path` used in results (never an absolute home path).
    pub(crate) fn display(&self, path: &Path) -> String {
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        real.strip_prefix(&self.root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| "<outside root>".into())
    }
}
