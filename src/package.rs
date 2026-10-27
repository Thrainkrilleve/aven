// src/package.rs — Aven Package Manager, Manifest & Unit-Verified Dependency Registry
// Milestone Phase 7: Ecosystem Bootstrap
#![allow(dead_code)]

use crate::evaluator::sha256_hex;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::units::DimensionalChecker;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct DependencySpec {
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeclaredUnitSpec {
    pub name: String,
    pub dimension: String,
    pub scale: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PackageManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub license: String,
    pub entry: String,
    pub dependencies: HashMap<String, DependencySpec>,
    pub units: Vec<DeclaredUnitSpec>,
}

impl Default for PackageManifest {
    fn default() -> Self {
        Self {
            name: "unnamed".to_string(),
            version: "0.1.0".to_string(),
            description: "An Aven model package".to_string(),
            authors: vec!["Anonymous <anon@example.com>".to_string()],
            license: "MIT".to_string(),
            entry: "lib.aven".to_string(),
            dependencies: HashMap::new(),
            units: Vec::new(),
        }
    }
}

impl PackageManifest {
    /// Formats manifest as clean Aven.toml string.
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("[package]\n");
        out.push_str(&format!("name = \"{}\"\n", self.name));
        out.push_str(&format!("version = \"{}\"\n", self.version));
        out.push_str(&format!("description = \"{}\"\n", self.description));
        let authors_str = self
            .authors
            .iter()
            .map(|a| format!("\"{}\"", a))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("authors = [{}]\n", authors_str));
        out.push_str(&format!("license = \"{}\"\n", self.license));
        out.push_str(&format!("entry = \"{}\"\n\n", self.entry));

        out.push_str("[dependencies]\n");
        let mut dep_keys: Vec<&String> = self.dependencies.keys().collect();
        dep_keys.sort();
        for k in dep_keys {
            let spec = &self.dependencies[k];
            if let Some(p) = &spec.path {
                out.push_str(&format!("{} = {{ path = \"{}\" }}\n", k, p));
            } else if let Some(v) = &spec.version {
                out.push_str(&format!("{} = \"{}\"\n", k, v));
            }
        }
        out.push('\n');

        if !self.units.is_empty() {
            for u in &self.units {
                out.push_str("[[units]]\n");
                out.push_str(&format!("name = \"{}\"\n", u.name));
                out.push_str(&format!("dimension = \"{}\"\n", u.dimension));
                out.push_str(&format!("scale = {}\n\n", u.scale));
            }
        }

        out
    }

    /// Pure-std parser for Aven.toml manifests.
    pub fn from_toml(content: &str) -> Result<Self, String> {
        let mut manifest = PackageManifest::default();
        let mut current_section = "package".to_string();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with("[[") && line.ends_with("]]") {
                let sec = &line[2..line.len() - 2].trim();
                current_section = format!("list_{}", sec);
                if *sec == "units" {
                    manifest.units.push(DeclaredUnitSpec {
                        name: String::new(),
                        dimension: String::new(),
                        scale: 1.0,
                    });
                }
                continue;
            } else if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].trim().to_string();
                continue;
            }

            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let val_str = line[eq_pos + 1..].trim();

                match current_section.as_str() {
                    "package" => match key {
                        "name" => manifest.name = parse_str_val(val_str)?,
                        "version" => manifest.version = parse_str_val(val_str)?,
                        "description" => manifest.description = parse_str_val(val_str)?,
                        "license" => manifest.license = parse_str_val(val_str)?,
                        "entry" => manifest.entry = parse_str_val(val_str)?,
                        "authors" => manifest.authors = parse_str_list(val_str)?,
                        _ => {}
                    },
                    "dependencies" => {
                        let dep_name = key.to_string();
                        if val_str.starts_with('{') && val_str.ends_with('}') {
                            // { path = "..." } or { version = "..." }
                            let inner = &val_str[1..val_str.len() - 1];
                            let mut path = None;
                            let mut ver = None;
                            for part in inner.split(',') {
                                if let Some(p) = part.find('=') {
                                    let k = part[..p].trim();
                                    let v = parse_str_val(part[p + 1..].trim())?;
                                    if k == "path" {
                                        path = Some(v);
                                    } else if k == "version" {
                                        ver = Some(v);
                                    }
                                }
                            }
                            manifest
                                .dependencies
                                .insert(dep_name, DependencySpec { version: ver, path });
                        } else {
                            let ver = parse_str_val(val_str)?;
                            manifest.dependencies.insert(
                                dep_name,
                                DependencySpec {
                                    version: Some(ver),
                                    path: None,
                                },
                            );
                        }
                    }
                    "list_units" => {
                        if let Some(last) = manifest.units.last_mut() {
                            match key {
                                "name" => last.name = parse_str_val(val_str)?,
                                "dimension" => last.dimension = parse_str_val(val_str)?,
                                "scale" => {
                                    last.scale =
                                        val_str.parse::<f64>().map_err(|e| e.to_string())?
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(manifest)
    }
}

fn parse_str_val(s: &str) -> Result<String, String> {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        Ok(s[1..s.len() - 1].to_string())
    } else {
        Ok(s.to_string())
    }
}

fn parse_str_list(s: &str) -> Result<Vec<String>, String> {
    let s = s.trim();
    if !s.starts_with('[') || !s.ends_with(']') {
        return Ok(vec![]);
    }
    let inner = &s[1..s.len() - 1];
    let items = inner
        .split(',')
        .map(|item| parse_str_val(item.trim()).unwrap_or_default())
        .filter(|s| !s.is_empty())
        .collect();
    Ok(items)
}

#[derive(Debug, Clone)]
pub struct PackageSummary {
    pub name: String,
    pub version: String,
    pub description: String,
    pub path: PathBuf,
}

pub struct PackageManager;

impl PackageManager {
    /// Initializes a new Aven package in target directory.
    pub fn init(dir: &Path, name: &str) -> Result<PathBuf, String> {
        if !dir.exists() {
            fs::create_dir_all(dir).map_err(|e| format!("failed to create package dir: {}", e))?;
        }

        let manifest_path = dir.join("Aven.toml");
        if manifest_path.exists() {
            return Err(format!(
                "package manifest already exists at '{:?}'",
                manifest_path
            ));
        }

        let manifest = PackageManifest {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: format!("Aven library package for {}", name),
            authors: vec!["Contributor <contrib@avenc.dev>".to_string()],
            license: "MIT".to_string(),
            entry: "lib.aven".to_string(),
            dependencies: HashMap::new(),
            units: vec![DeclaredUnitSpec {
                name: "kilo_watt".to_string(),
                dimension: "Power".to_string(),
                scale: 1000.0,
            }],
        };

        fs::write(&manifest_path, manifest.to_toml())
            .map_err(|e| format!("failed to write Aven.toml: {}", e))?;

        let entry_path = dir.join(&manifest.entry);
        if !entry_path.exists() {
            let sample_code = format!(
                "// {} — Aven Unit-Verified Relation Library\n\
                 relation {}Relations {{\n\
                     power_kw = (voltage_v * current_a) / 1000\n\
                     energy_kwh = power_kw * operating_hours\n\
                 }}\n",
                name, name
            );
            fs::write(&entry_path, sample_code)
                .map_err(|e| format!("failed to write entry source: {}", e))?;
        }

        Ok(manifest_path)
    }

    /// Bundles an Aven package into an `.avenpkg` distribution archive with cryptographic SHA256 seal.
    pub fn pack(dir: &Path, out_file: Option<&Path>) -> Result<(PathBuf, String), String> {
        let manifest_path = dir.join("Aven.toml");
        if !manifest_path.exists() {
            return Err(format!("no Aven.toml found in '{:?}'", dir));
        }

        let manifest_str = fs::read_to_string(&manifest_path)
            .map_err(|e| format!("failed to read Aven.toml: {}", e))?;
        let manifest = PackageManifest::from_toml(&manifest_str)?;

        // Verify entry file exists and passes dimensional checking
        let entry_path = dir.join(&manifest.entry);
        if !entry_path.exists() {
            return Err(format!(
                "entry file '{:?}' does not exist in package",
                entry_path
            ));
        }
        let entry_src = fs::read_to_string(&entry_path)
            .map_err(|e| format!("failed to read entry source: {}", e))?;

        let tokens = Lexer::new(&entry_src)
            .tokenize()
            .map_err(|e| format!("tokenizer error in package entry: {}", e))?;
        let module = Parser::new(tokens)
            .parse_module(&manifest.name)
            .map_err(|e| format!("parse error in package entry: {}", e))?;

        let mut type_checker = DimensionalChecker::new();
        type_checker
            .check_module(&module)
            .map_err(|e| format!("dimensional verification failed in package: {}", e))?;

        // Format archive:
        // Header: AVENPKG1\n
        // Manifest length + manifest content
        // Number of files
        // For each file: rel_path + length + content
        // Footer: SHA256 of payload
        let mut payload = Vec::new();
        payload.extend_from_slice(b"AVENPKG1\n");

        let m_bytes = manifest_str.as_bytes();
        payload.extend_from_slice(&(m_bytes.len() as u64).to_le_bytes());
        payload.extend_from_slice(m_bytes);

        // Collect all .aven files in dir
        let mut files = Vec::new();
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.is_file() && p.extension().map_or(false, |ext| ext == "aven") {
                let rel = p.file_name().unwrap().to_str().unwrap().to_string();
                let bytes = fs::read(&p).map_err(|e| e.to_string())?;
                files.push((rel, bytes));
            }
        }

        payload.extend_from_slice(&(files.len() as u64).to_le_bytes());
        for (name, content) in files {
            let n_bytes = name.as_bytes();
            payload.extend_from_slice(&(n_bytes.len() as u64).to_le_bytes());
            payload.extend_from_slice(n_bytes);
            payload.extend_from_slice(&(content.len() as u64).to_le_bytes());
            payload.extend_from_slice(&content);
        }

        let digest = sha256_hex(&payload);
        payload.extend_from_slice(digest.as_bytes());

        let target_path = match out_file {
            Some(p) => p.to_path_buf(),
            None => dir.join(format!("{}-{}.avenpkg", manifest.name, manifest.version)),
        };

        fs::write(&target_path, &payload)
            .map_err(|e| format!("failed to write package archive: {}", e))?;

        Ok((target_path, digest))
    }

    /// Verifies and unpacks an `.avenpkg` archive into a local repository / cache directory.
    pub fn install(archive_path: &Path, cache_dir: &Path) -> Result<PackageManifest, String> {
        let bytes = fs::read(archive_path)
            .map_err(|e| format!("failed to read archive '{:?}': {}", archive_path, e))?;

        if bytes.len() < 73 || &bytes[..9] != b"AVENPKG1\n" {
            return Err("invalid AVENPKG archive header".to_string());
        }

        let payload_len = bytes.len() - 64;
        let payload = &bytes[..payload_len];
        let expected_digest = String::from_utf8_lossy(&bytes[payload_len..]);
        let actual_digest = sha256_hex(payload);

        if expected_digest.trim() != actual_digest {
            return Err(format!(
                "package integrity check failed: expected hash {} but calculated {}",
                expected_digest, actual_digest
            ));
        }

        let mut offset = 9;
        let m_len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
        offset += 8;
        let manifest_str = String::from_utf8_lossy(&bytes[offset..offset + m_len]).to_string();
        offset += m_len;

        let manifest = PackageManifest::from_toml(&manifest_str)?;
        let install_dir = cache_dir.join(&manifest.name).join(&manifest.version);
        fs::create_dir_all(&install_dir)
            .map_err(|e| format!("failed to create install dir: {}", e))?;

        fs::write(install_dir.join("Aven.toml"), &manifest_str)
            .map_err(|e| format!("failed to write manifest: {}", e))?;

        let num_files = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
        offset += 8;

        for _ in 0..num_files {
            let n_len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
            offset += 8;
            let fname = String::from_utf8_lossy(&bytes[offset..offset + n_len]).to_string();
            offset += n_len;

            let c_len = u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as usize;
            offset += 8;
            let content = &bytes[offset..offset + c_len];
            offset += c_len;

            fs::write(install_dir.join(&fname), content)
                .map_err(|e| format!("failed to extract file '{}': {}", fname, e))?;
        }

        Ok(manifest)
    }

    /// Lists installed packages in cache directory.
    pub fn list(cache_dir: &Path) -> Result<Vec<PackageSummary>, String> {
        let mut summaries = Vec::new();
        if !cache_dir.exists() {
            return Ok(summaries);
        }

        for pkg_entry in fs::read_dir(cache_dir).map_err(|e| e.to_string())? {
            let pkg_dir = pkg_entry.map_err(|e| e.to_string())?.path();
            if pkg_dir.is_dir() {
                for ver_entry in fs::read_dir(&pkg_dir).map_err(|e| e.to_string())? {
                    let ver_dir = ver_entry.map_err(|e| e.to_string())?.path();
                    let toml_file = ver_dir.join("Aven.toml");
                    if toml_file.exists() {
                        if let Ok(content) = fs::read_to_string(&toml_file) {
                            if let Ok(m) = PackageManifest::from_toml(&content) {
                                summaries.push(PackageSummary {
                                    name: m.name,
                                    version: m.version,
                                    description: m.description,
                                    path: ver_dir,
                                });
                            }
                        }
                    }
                }
            }
        }

        summaries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(summaries)
    }

    /// Resolves `import <package_name>;` directives by locating the package in search dirs
    /// and inlining its verified relations into the model source.
    pub fn resolve_imports(source: &str, search_dirs: &[PathBuf]) -> Result<String, String> {
        let mut output = String::new();
        for line in source.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("import ") && trimmed.ends_with(';') {
                let pkg_name = trimmed[7..trimmed.len() - 1].trim();
                let mut found = false;

                for dir in search_dirs {
                    // Check direct path or pkg_name subdirectory
                    let mut candidate_paths = vec![dir.join(pkg_name).join("Aven.toml")];
                    // `avenc pkg install` lays packages out as <name>/<version>/; newest first
                    candidate_paths.extend(
                        installed_versions(&dir.join(pkg_name))
                            .into_iter()
                            .map(|v| v.join("Aven.toml")),
                    );
                    candidate_paths.push(dir.join(pkg_name).join("lib.aven"));
                    candidate_paths.push(dir.join(format!("{}.aven", pkg_name)));
                    if dir.join("Aven.toml").exists() {
                        candidate_paths.push(dir.join("Aven.toml"));
                    }

                    for cp in &candidate_paths {
                        if cp.exists() {
                            let src_to_inline = if cp.file_name().unwrap() == "Aven.toml" {
                                let m_str = match fs::read_to_string(cp) {
                                    Ok(s) => s,
                                    Err(_) => continue,
                                };
                                let manifest = match PackageManifest::from_toml(&m_str) {
                                    Ok(m) => m,
                                    Err(_) => continue,
                                };
                                if manifest.name != pkg_name {
                                    continue;
                                }
                                let entry = cp.parent().unwrap().join(&manifest.entry);
                                fs::read_to_string(&entry).map_err(|e| e.to_string())?
                            } else {
                                fs::read_to_string(cp).map_err(|e| e.to_string())?
                            };

                            output.push_str(&format!(
                                "// --- Inlined from package '{}' ({:?}) ---\n",
                                pkg_name, cp
                            ));
                            output.push_str(&src_to_inline);
                            output.push('\n');
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }

                if !found {
                    return Err(format!(
                        "cannot resolve import '{}': package not found in search paths",
                        pkg_name
                    ));
                }
            } else {
                output.push_str(line);
                output.push('\n');
            }
        }
        Ok(output)
    }
}

/// Version directories under an installed package's directory, highest version first.
fn installed_versions(pkg_dir: &std::path::Path) -> Vec<PathBuf> {
    let parse = |name: &str| -> Option<Vec<u64>> {
        name.split('.').map(|p| p.parse::<u64>().ok()).collect()
    };
    let mut versions: Vec<(Vec<u64>, PathBuf)> = match fs::read_dir(pkg_dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let v = parse(&e.file_name().to_string_lossy())?;
                Some((v, e.path()))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    versions.sort_by(|a, b| b.0.cmp(&a.0));
    versions.into_iter().map(|(_, p)| p).collect()
}
