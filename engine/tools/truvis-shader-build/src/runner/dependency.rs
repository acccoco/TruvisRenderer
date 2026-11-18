//! Shader 源码和编译器依赖校验。
//!
//! 源码预检约束 canonical include 路径、唯一解析和 `abi <- lib <- entry` 层级；
//! 编译完成后的 depfile 校验真实传递依赖，避免条件 include 或间接 include 绕过 package 边界。

use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

use truvis_shader_manifest::ShaderSourceLayer;

use super::common::ShaderCompileTask;

/// 一个需要执行源码预检的 owner 根目录。
pub struct ShaderSourceRoot {
    pub path: PathBuf,
    pub layer: SourceLayer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceLayer {
    Abi,
    Lib,
    Entry,
}

impl From<ShaderSourceLayer> for SourceLayer {
    fn from(value: ShaderSourceLayer) -> Self {
        match value {
            ShaderSourceLayer::Abi => Self::Abi,
            ShaderSourceLayer::Lib => Self::Lib,
        }
    }
}

/// package 通过自身或依赖 package 可见的共享源码根。
pub struct ShaderDependencyRoot {
    pub path: PathBuf,
    pub layer: SourceLayer,
}

/// 单个 package 的源码可见范围。
pub struct ShaderSourcePackage {
    pub package_id: String,
    pub include_roots: Vec<PathBuf>,
    pub source_roots: Vec<ShaderSourceRoot>,
    pub allowed_dependency_roots: Vec<ShaderDependencyRoot>,
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
        let packages = packages
            .iter()
            .map(|package| {
                Ok(ShaderSourcePackage {
                    package_id: package.package_id.clone(),
                    include_roots: self.canonicalize_paths(&package.include_roots, &package.package_id)?,
                    source_roots: package
                        .source_roots
                        .iter()
                        .map(|root| {
                            Ok(ShaderSourceRoot {
                                path: Self::canonicalize(&root.path, "shader source root")?,
                                layer: root.layer,
                            })
                        })
                        .collect::<Result<_, String>>()?,
                    allowed_dependency_roots: self
                        .canonicalize_dependency_roots(&package.allowed_dependency_roots, &package.package_id)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut errors = Vec::new();
        for package in &packages {
            let mut visited = BTreeSet::new();
            for source_root in &package.source_roots {
                for entry in walkdir::WalkDir::new(&source_root.path) {
                    let entry = entry
                        .map_err(|err| format!("无法遍历 shader 预检目录 {}: {err}", source_root.path.display()))?;
                    if !entry.path().is_file() || !Self::is_shader_source(entry.path()) {
                        continue;
                    }
                    if let Err(err) =
                        self.validate_source_file(&packages, package, entry.path(), source_root.layer, &mut visited)
                    {
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
        let allowed_roots = self.canonicalize_paths(&task.allowed_dependency_roots, &task.package_id)?;

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
        packages: &[ShaderSourcePackage],
        package: &ShaderSourcePackage,
        source_path: &Path,
        source_layer: SourceLayer,
        visited: &mut BTreeSet<PathBuf>,
    ) -> Result<(), String> {
        let source_path = Self::canonicalize(source_path, "shader source")?;
        if !visited.insert(source_path.clone()) {
            return Ok(());
        }
        let content = std::fs::read_to_string(&source_path)
            .map_err(|err| format!("无法读取 shader 源码 {}: {err}", source_path.display()))?;
        let mut errors = Vec::new();

        for (line_number, line) in ShaderIncludeDirective::logical_lines(&content) {
            let location = format!("{}:{}", source_path.display(), line_number);
            let include_path = match ShaderIncludeDirective::parse(&line) {
                Ok(Some(path)) => path,
                Ok(None) => continue,
                Err(err) => {
                    errors.push(format!("{location}: {err}"));
                    continue;
                }
            };
            if let Err(err) = Self::validate_include_path(&include_path) {
                errors.push(format!("{location}: {err}"));
                continue;
            }

            let candidates = package
                .include_roots
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
            let local = source_path.parent().expect("canonical source has parent").join(&include_path);
            if local.is_file() && Self::canonicalize(&local, "source-relative include")? != resolved {
                errors.push(format!(
                    "{location}: 源文件相对路径遮蔽 include root: '{include_path}' -> {}",
                    local.display()
                ));
                continue;
            }
            if !resolved.starts_with(&self.workspace_root) {
                errors.push(format!("{location}: include 位于 workspace 外: {}", resolved.display()));
                continue;
            }
            let Some(target_root) =
                package.allowed_dependency_roots.iter().find(|root| resolved.starts_with(&root.path))
            else {
                errors.push(format!(
                    "{location}: package '{}' 未声明 include 依赖: {}",
                    package.package_id,
                    resolved.display()
                ));
                continue;
            };
            if source_layer == SourceLayer::Abi && target_root.layer != SourceLayer::Abi {
                errors.push(format!("{location}: ABI 只能依赖 ABI，不能引用 '{include_path}'"));
                continue;
            }
            let mut owners = packages.iter().filter(|owner| {
                owner.source_roots.iter().any(|root| root.path == target_root.path && root.layer == target_root.layer)
            });
            let owner =
                owners.next().ok_or_else(|| format!("{}: include 缺少声明 owner: {}", location, resolved.display()))?;
            if owners.next().is_some() {
                return Err(format!("{location}: include 存在多个声明 owner: {}", resolved.display()));
            }
            // 实际引用的文本无论扩展名都必须逐边检查，depfile 无法恢复引用方的 layer。
            if let Err(err) = self.validate_source_file(packages, owner, &resolved, target_root.layer, visited) {
                errors.push(err);
            }
        }

        if errors.is_empty() { Ok(()) } else { Err(errors.join("\n")) }
    }

    fn validate_include_path(include_path: &str) -> Result<(), String> {
        if include_path.contains('\\') {
            return Err(format!("include 必须使用 '/' 分隔: '{include_path}'"));
        }
        let path = Path::new(include_path);
        if include_path.is_empty()
            || include_path.split('/').any(|part| matches!(part, "" | "." | ".."))
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::CurDir | Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!("include 必须是 canonical 相对路径: '{include_path}'"));
        }
        let mut parts = include_path.split('/');
        if !matches!(parts.next(), Some("abi" | "lib")) || parts.next().is_none() || parts.next().is_none() {
            return Err(format!("include 必须保留 abi/lib 与 owner 前缀: '{include_path}'"));
        }

        Ok(())
    }

    fn canonicalize_paths(&self, roots: &[PathBuf], package_id: &str) -> Result<Vec<PathBuf>, String> {
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

    fn canonicalize_dependency_roots(
        &self,
        roots: &[ShaderDependencyRoot],
        package_id: &str,
    ) -> Result<Vec<ShaderDependencyRoot>, String> {
        roots
            .iter()
            .map(|root| {
                let canonical = Self::canonicalize(&root.path, "shader dependency root")?;
                if !canonical.starts_with(&self.workspace_root) {
                    return Err(format!(
                        "shader package '{}' 的依赖根位于 workspace 外: {}",
                        package_id,
                        canonical.display()
                    ));
                }
                Ok(ShaderDependencyRoot {
                    path: canonical,
                    layer: root.layer,
                })
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

/// 只接受可在构建前确定逐边依赖的字面量 include/import，不求值宏或条件分支。
struct ShaderIncludeDirective;

impl ShaderIncludeDirective {
    fn parse(line: &str) -> Result<Option<String>, String> {
        let trimmed = line.trim_start();
        let (directive, preprocessor) = if let Some(after_hash) = trimmed.strip_prefix('#') {
            (after_hash.trim_start(), true)
        } else {
            (trimmed, false)
        };
        let keyword_end = directive.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(directive.len());
        let keyword = &directive[..keyword_end];
        if !matches!((preprocessor, keyword), (true, "include") | (false, "import")) {
            return Ok(None);
        }
        let argument = directive[keyword_end..].trim();
        let invalid = || "include/import 必须使用完整字面量路径；宏或命名 module 无法静态验证逐边层级".to_string();

        let (terminator, body) = match argument.chars().next() {
            Some('"') => ('"', &argument[1..]),
            Some('<') => ('>', &argument[1..]),
            _ => return Err(invalid()),
        };
        let end = body.find(terminator).ok_or_else(invalid)?;
        let trailing = body[end + 1..].trim();
        if !(trailing.is_empty() || (!preprocessor && trailing == ";")) {
            return Err(invalid());
        }
        Ok(Some(body[..end].to_string()))
    }

    /// 先移除续行，再将注释视为空白；保留字符串内容和每个逻辑行的原始起始行号。
    /// 不展开条件编译：未启用分支也必须满足项目依赖约束。
    fn logical_lines(content: &str) -> Vec<(usize, String)> {
        let mut spliced = Vec::new();
        let mut chars = content.trim_start_matches('\u{feff}').chars().peekable();
        let mut line = 1;
        while let Some(c) = chars.next() {
            if c == '\\' && matches!(chars.peek(), Some('\n' | '\r')) {
                if chars.next() == Some('\r') && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line += 1;
                continue;
            }
            spliced.push((line, c));
            if c == '\n' {
                line += 1;
            }
        }
        let mut result = Vec::new();
        let mut current = String::new();
        let mut start_line = 1;
        let mut block_comment = false;
        let mut line_comment = false;
        let mut quote = None;
        let mut chars = spliced.into_iter().peekable();
        while let Some((line, c)) = chars.next() {
            if block_comment {
                if c == '*' && chars.peek().is_some_and(|(_, next)| *next == '/') {
                    chars.next();
                    block_comment = false;
                }
                continue;
            }
            if c == '\n' {
                result.push((start_line, std::mem::take(&mut current)));
                start_line = line + 1;
                line_comment = false;
                quote = None;
                continue;
            }
            if line_comment {
                continue;
            }
            if let Some(delimiter) = quote {
                current.push(c);
                if c == '\\' {
                    if let Some((_, escaped)) = chars.next() {
                        current.push(escaped);
                    }
                } else if c == delimiter {
                    quote = None;
                }
                continue;
            }
            if c == '/' && chars.peek().is_some_and(|(_, next)| matches!(next, '/' | '*')) {
                block_comment = chars.next().is_some_and(|(_, next)| next == '*');
                line_comment = !block_comment;
                current.push(' ');
                continue;
            }
            if matches!(c, '"' | '\'') {
                quote = Some(c);
            }
            current.push(c);
        }
        result.push((start_line, current));
        result
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
