//! Shader 源码和编译器依赖校验。
//!
//! 源码预检约束 canonical include 路径、唯一解析和 `abi <- lib <- entry` 层级；
//! 编译完成后的 depfile 校验真实传递依赖，避免条件 include 或间接 include 绕过 package 边界。

use std::path::{Component, Path, PathBuf};

use super::common::ShaderCompileTask;

/// Shader 源文件所在的职责层级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShaderSourceLayer {
    Abi,
    Lib,
    Entry,
}

/// 一个需要执行源码预检的 owner 根目录。
pub struct ShaderSourceRoot {
    pub path: PathBuf,
    pub layer: ShaderSourceLayer,
}

/// 单个 package 的源码可见范围。
pub struct ShaderSourcePackage {
    pub package_id: String,
    pub include_roots: Vec<PathBuf>,
    pub source_roots: Vec<ShaderSourceRoot>,
    pub allowed_dependency_roots: Vec<PathBuf>,
}

/// 统一执行源码静态检查和编译器 depfile 检查。
pub struct ShaderDependencyValidator {
    workspace_root: PathBuf,
}

impl ShaderDependencyValidator {
    pub fn new(workspace_root: &Path) -> Result<Self, String> {
        Ok(Self {
            workspace_root: Self::canonicalize(workspace_root, "shader workspace")?,
        })
    }

    /// 在调用编译器前检查所有 owner 源码，尽早给出具体 include 位置和错误原因。
    pub fn validate_sources(&self, packages: &[ShaderSourcePackage]) -> Result<(), String> {
        let mut errors = Vec::new();
        for package in packages {
            let include_roots = match self.canonicalize_roots(&package.include_roots, &package.package_id) {
                Ok(roots) => roots,
                Err(err) => {
                    errors.push(err);
                    continue;
                }
            };
            let allowed_roots = match self.canonicalize_roots(&package.allowed_dependency_roots, &package.package_id) {
                Ok(roots) => roots,
                Err(err) => {
                    errors.push(err);
                    continue;
                }
            };

            for source_root in &package.source_roots {
                for entry in walkdir::WalkDir::new(&source_root.path).into_iter().filter_map(Result::ok) {
                    if !entry.path().is_file() || !Self::is_shader_source(entry.path()) {
                        continue;
                    }
                    if let Err(err) = self.validate_source_file(
                        &package.package_id,
                        entry.path(),
                        source_root.layer,
                        &include_roots,
                        &allowed_roots,
                    ) {
                        errors.push(err);
                    }
                }
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    /// 编译成功后以编译器实际输出的 Makefile depfile 为准检查传递依赖。
    pub fn validate_compiler_dependencies(&self, task: &ShaderCompileTask) -> Result<(), String> {
        let content = std::fs::read_to_string(&task.depfile_path).map_err(|err| {
            format!(
                "无法读取 shader depfile: package={} source={} depfile={} error={err}",
                task.package_id,
                task.shader_path.display(),
                task.depfile_path.display()
            )
        })?;
        let dependencies = MakefileDependencyParser::parse(&content).map_err(|err| {
            format!(
                "无法解析 shader depfile: package={} source={} depfile={} error={err}",
                task.package_id,
                task.shader_path.display(),
                task.depfile_path.display()
            )
        })?;
        let entry_path = Self::canonicalize(&task.shader_path, "shader entry")?;
        let allowed_roots = self.canonicalize_roots(&task.allowed_dependency_roots, &task.package_id)?;

        let mut rejected = Vec::new();
        let mut contains_entry = false;
        for dependency in dependencies {
            let absolute_path =
                if dependency.is_absolute() { dependency } else { self.workspace_root.join(dependency) };
            let dependency_path = match Self::canonicalize(&absolute_path, "shader dependency") {
                Ok(path) => path,
                Err(err) => {
                    rejected.push(err);
                    continue;
                }
            };
            if !dependency_path.starts_with(&self.workspace_root) {
                rejected.push(format!("依赖位于 workspace 外: {}", dependency_path.display()));
                continue;
            }
            if dependency_path == entry_path {
                contains_entry = true;
                continue;
            }
            if allowed_roots.iter().any(|root| dependency_path.starts_with(root)) {
                continue;
            }
            rejected.push(format!("未声明的依赖: {}", dependency_path.display()));
        }
        if !contains_entry {
            rejected.push("depfile 未声明当前 shader entry".to_string());
        }

        if rejected.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Shader package 依赖越界: package={} source={}\n{}",
                task.package_id,
                task.shader_path.display(),
                rejected.join("\n")
            ))
        }
    }

    fn validate_source_file(
        &self,
        package_id: &str,
        source_path: &Path,
        source_layer: ShaderSourceLayer,
        include_roots: &[PathBuf],
        allowed_roots: &[PathBuf],
    ) -> Result<(), String> {
        let content = std::fs::read_to_string(source_path)
            .map_err(|err| format!("无法读取 shader 源码 {}: {err}", source_path.display()))?;
        let mut errors = Vec::new();

        for (line_index, line) in content.lines().enumerate() {
            let Some(include_path) = ShaderIncludeDirective::parse(line) else {
                continue;
            };
            let location = format!("{}:{}", source_path.display(), line_index + 1);
            let target_layer = match Self::validate_include_path(&include_path) {
                Ok(layer) => layer,
                Err(err) => {
                    errors.push(format!("{location}: {err}"));
                    continue;
                }
            };
            if source_layer == ShaderSourceLayer::Abi && target_layer != ShaderSourceLayer::Abi {
                errors.push(format!("{location}: ABI 只能依赖 ABI，不能引用 '{include_path}'"));
                continue;
            }

            let candidates = include_roots
                .iter()
                .map(|root| root.join(&include_path))
                .filter(|path| path.is_file())
                .collect::<Vec<_>>();
            if candidates.is_empty() {
                errors.push(format!("{location}: include 无法解析: '{include_path}'"));
                continue;
            }
            if candidates.len() != 1 {
                errors.push(format!(
                    "{location}: include 必须唯一解析: '{}' -> {}",
                    include_path,
                    candidates.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join(", ")
                ));
                continue;
            }

            let resolved = match Self::canonicalize(&candidates[0], "shader include") {
                Ok(path) => path,
                Err(err) => {
                    errors.push(format!("{location}: {err}"));
                    continue;
                }
            };
            if !resolved.starts_with(&self.workspace_root) {
                errors.push(format!("{location}: include 位于 workspace 外: {}", resolved.display()));
                continue;
            }
            if !allowed_roots.iter().any(|root| resolved.starts_with(root)) {
                errors.push(format!(
                    "{location}: package '{}' 未声明 include 依赖: {}",
                    package_id,
                    resolved.display()
                ));
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    fn validate_include_path(include_path: &str) -> Result<ShaderSourceLayer, String> {
        if include_path.contains('\\') {
            return Err(format!("include 必须使用 '/' 分隔: '{include_path}'"));
        }
        let path = Path::new(include_path);
        if include_path.is_empty()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!("include 必须是 canonical 相对路径: '{include_path}'"));
        }

        if include_path.starts_with("abi/engine/") || include_path.starts_with("abi/renderer/") {
            Ok(ShaderSourceLayer::Abi)
        } else if include_path.starts_with("lib/engine/")
            || include_path.starts_with("lib/renderer/")
            || include_path.starts_with("lib/sample-shader-toy/")
        {
            Ok(ShaderSourceLayer::Lib)
        } else {
            Err(format!(
                "include 必须带 layer/owner 前缀（abi/engine、abi/renderer、lib/engine、lib/renderer 或 lib/sample-shader-toy）: '{include_path}'"
            ))
        }
    }

    fn canonicalize_roots(&self, roots: &[PathBuf], package_id: &str) -> Result<Vec<PathBuf>, String> {
        roots
            .iter()
            .map(|root| {
                let canonical = Self::canonicalize(root, "shader dependency root")?;
                if !canonical.starts_with(&self.workspace_root) {
                    return Err(format!(
                        "shader package '{}' 的依赖根位于 workspace 外: {}",
                        package_id,
                        canonical.display()
                    ));
                }
                Ok(canonical)
            })
            .collect()
    }

    fn canonicalize(path: &Path, label: &str) -> Result<PathBuf, String> {
        std::fs::canonicalize(path).map_err(|err| format!("无法规范化 {label} {}: {err}", path.display()))
    }

    fn is_shader_source(path: &Path) -> bool {
        matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some(
                "slang"
                    | "slangi"
                    | "glsl"
                    | "vert"
                    | "frag"
                    | "comp"
                    | "rgen"
                    | "rchit"
                    | "rmiss"
                    | "rahit"
                    | "rint"
                    | "rcall"
                    | "tesc"
                    | "tese"
                    | "geom"
                    | "task"
                    | "mesh"
                    | "hlsl"
            )
        )
    }
}

/// 只解析字面量 `#include` 与字面量 `import`；宏 include 交给编译器 depfile 兜底。
struct ShaderIncludeDirective;

impl ShaderIncludeDirective {
    fn parse(line: &str) -> Option<String> {
        let trimmed = line.trim_start();
        let argument = if let Some(after_hash) = trimmed.strip_prefix('#') {
            after_hash.trim_start().strip_prefix("include")?.trim_start()
        } else {
            trimmed.strip_prefix("import")?.trim_start()
        };

        let (terminator, body) = match argument.chars().next()? {
            '"' => ('"', &argument[1..]),
            '<' => ('>', &argument[1..]),
            _ => return None,
        };
        let end = body.find(terminator)?;
        Some(body[..end].to_string())
    }
}

/// 解析 Slang、GLSLC 与 DXC 共同使用的 Makefile dependency 语法。
struct MakefileDependencyParser;

impl MakefileDependencyParser {
    fn parse(content: &str) -> Result<Vec<PathBuf>, String> {
        let dependency_start =
            Self::find_rule_separator(content).ok_or_else(|| "depfile 缺少未转义的规则分隔符 ':'".to_string())?;
        let tokens = Self::parse_tokens(&content[dependency_start + 1..]);
        if tokens.is_empty() {
            return Err("depfile 没有声明任何依赖".to_string());
        }
        Ok(tokens.into_iter().map(PathBuf::from).collect())
    }

    fn find_rule_separator(content: &str) -> Option<usize> {
        let bytes = content.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index += 2,
                b':' if bytes.get(index + 1).is_none_or(u8::is_ascii_whitespace) => return Some(index),
                _ => index += 1,
            }
        }
        None
    }

    fn parse_tokens(content: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut chars = content.chars().peekable();
        while let Some(character) = chars.next() {
            if character == '\\' {
                match chars.peek().copied() {
                    Some('\r') => {
                        chars.next();
                        if chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                    }
                    Some('\n') => {
                        chars.next();
                    }
                    Some(next @ ('\\' | ' ' | '\t' | '#' | ':')) => {
                        chars.next();
                        current.push(next);
                    }
                    Some(next) => {
                        chars.next();
                        current.push('\\');
                        current.push(next);
                    }
                    None => current.push('\\'),
                }
            } else if character.is_whitespace() {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            } else {
                current.push(character);
            }
        }
        if !current.is_empty() {
            tokens.push(current);
        }
        tokens
    }
}
