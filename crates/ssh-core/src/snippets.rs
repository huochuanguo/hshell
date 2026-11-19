use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Write, path::Path};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub group: String,
    pub command: String,
}

pub fn validate_snippet_text(command: &str) -> Result<(), String> {
    if command.trim().is_empty() || command.len() > 4096 || command.chars().any(char::is_control) {
        return Err("命令须为单行文本，不能含控制字符，最多 4096 字节".into());
    }
    Ok(())
}

fn validate(snippets: &[Snippet]) -> Result<(), String> {
    if snippets.len() > 200 {
        return Err("最多保存 200 条命令片段".into());
    }
    let mut ids = HashSet::new();
    for snippet in snippets {
        if uuid::Uuid::parse_str(&snippet.id).is_err() || !ids.insert(&snippet.id) {
            return Err("命令片段标识无效或重复".into());
        }
        if snippet.name.trim().is_empty()
            || snippet.name.chars().count() > 80
            || snippet.name.chars().any(char::is_control)
            || snippet.group.chars().count() > 40
            || snippet.group.chars().any(char::is_control)
        {
            return Err("命令名称须为 1–80 个字符，分组最多 40 个字符".into());
        }
        validate_snippet_text(&snippet.command)?;
    }
    Ok(())
}

pub(crate) fn read_bounded(file: std::fs::File) -> Result<Vec<u8>, String> {
    use std::io::Read;
    const LIMIT: u64 = 4 * 1024 * 1024;
    let mut data = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() as u64 > LIMIT {
        return Err("配置文件超过 4 MiB 限制".into());
    }
    Ok(data)
}

pub fn load_snippets(path: &Path) -> Result<Vec<Snippet>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("读取命令片段失败：{error}")),
    };
    let snippets: Vec<Snippet> = serde_json::from_slice(&read_bounded(file)?)
        .map_err(|error| format!("命令片段文件损坏：{error}"))?;
    validate(&snippets)?;
    Ok(snippets)
}

pub fn save_snippets(path: &Path, snippets: &[Snippet]) -> Result<(), String> {
    validate(snippets)?;
    // 先检查原文件，避免空白界面覆盖损坏的命令库。
    load_snippets(path)?;
    let parent = path.parent().ok_or("命令片段目录无效")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer_pretty(&mut file, snippets).map_err(|e| e.to_string())?;
    file.flush()
        .and_then(|()| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippets_roundtrip_and_reject_execution_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("commands.json");
        let mut snippets = vec![Snippet {
            id: uuid::Uuid::new_v4().to_string(),
            name: "文件列表".into(),
            group: "日常".into(),
            command: "ls -l './目录名'".into(),
        }];
        save_snippets(&path, &snippets).unwrap();
        let original = std::fs::read(&path).unwrap();
        assert_eq!(
            load_snippets(&path).unwrap()[0].command,
            snippets[0].command
        );
        for input in ["ls\n", "ls\r", "ls\u{1b}", "ls\t", "ls\u{0085}", "   "] {
            snippets[0].command = input.into();
            assert!(save_snippets(&path, &snippets).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
        std::fs::write(&path, b"broken").unwrap();
        assert!(save_snippets(&path, &[]).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
    }
}
