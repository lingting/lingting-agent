//! 系统提示词来源：`AGENT_HOME` 下的 `AGENTS.md`。
//!
//! 匹配顺序（P2-020 定稿，用户口径）：
//!
//! 1. 名称全大写、扩展名不区分大小写的 `AGENTS.md`（如 `AGENTS.md` / `AGENTS.MD`）；
//! 2. 其余忽略大小写后名称匹配的 Markdown 文件（如 `agents.md` / `Agents.MD`）。
//!
//! 同一档位有多个候选时按文件名排序取第一个；一个都找不到（或内容为空）时系统提示词为空
//! 字符串，组装阶段跳过系统消息，不发送空消息。
//!
//! 每次组装都现场扫描并读取，不做缓存：改完提示词文件后，下一次执行即生效，不需要重启服务、
//! 也不需要在装配期预热。

use std::ffi::OsStr;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use agent_context_core::ContextError;

/// 系统提示词的文件名主干。
const SYSTEM_PROMPT_STEM: &str = "AGENTS";
/// 只接受 Markdown 文件；扩展名不区分大小写。
const SYSTEM_PROMPT_EXTENSION: &str = "md";

/// 系统提示词来源目录（应用根目录 `AGENT_HOME`）。
#[derive(Debug, Clone)]
pub struct SystemPrompt {
    /// 扫描目录。
    directory: PathBuf,
}

impl SystemPrompt {
    /// 以应用根目录构造。
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// 现场匹配并读取提示词；没有可用文件时返回空字符串。
    pub fn load(&self) -> Result<String, ContextError> {
        let Some(path) = self.locate()? else {
            tracing::info!(
                directory = %self.directory.display(),
                "未找到系统提示词文件，本次不发送系统消息"
            );
            return Ok(String::new());
        };

        let content = fs::read_to_string(&path).map_err(|source| ContextError::SystemPrompt {
            path: path.clone(),
            source,
        })?;
        tracing::info!(
            file = %path.display(),
            chars = content.chars().count(),
            "已加载系统提示词文件"
        );
        Ok(content)
    }

    /// 按匹配顺序挑出提示词文件；目录不存在等同「没有文件」。
    fn locate(&self) -> Result<Option<PathBuf>, ContextError> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(ContextError::SystemPrompt {
                    path: self.directory.clone(),
                    source,
                });
            }
        };

        // 第一元组项是「名称全大写」标记，排序时置前；第二项保证同档位可复现。
        let mut candidates: Vec<(bool, String, PathBuf)> = Vec::new();
        for entry in entries {
            let path = entry
                .map_err(|source| ContextError::SystemPrompt {
                    path: self.directory.clone(),
                    source,
                })?
                .path();
            if !is_markdown(&path) {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(OsStr::to_str) else {
                continue;
            };
            if !stem.eq_ignore_ascii_case(SYSTEM_PROMPT_STEM) {
                continue;
            }
            let name = path.file_name().and_then(OsStr::to_str).unwrap_or_default().to_owned();
            candidates.push((stem == SYSTEM_PROMPT_STEM, name, path));
        }

        candidates.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
        Ok(candidates.into_iter().next().map(|(_, _, path)| path))
    }
}

/// 是否为 Markdown 文件（扩展名不区分大小写，且必须是普通文件）。
fn is_markdown(path: &Path) -> bool {
    let markdown = path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SYSTEM_PROMPT_EXTENSION));
    markdown && path.is_file()
}
