//! Docker-hosted MSVC 2002 build provider.
//!
//! Wraps `cmkr gen` + `cmake --build` inside a Docker container that
//! hosts the MSVC 2002 (Visual Studio 6) toolchain. All configuration
//! (image name, generator, toolchain path) comes from the constructor —
//! nothing is hard-coded in the trait implementation.
//!
//! Every Docker command is validated against an allowlist before
//! execution: project-root containment, configured image name only,
//! no shell metacharacters in arguments.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use tokio::process::Command;

use super::diagnostics::parse_msvc_diagnostics;
use super::trait_def::{BuildProviderError, BuildProviderTrait, BuildResult};
use super::types::{
    BuildConfigured, BuildDiagnostic, BuildLogs, CompileResult, CompileUnit, GeneratorManifest,
    LinkResult, RunTestResult,
};

/// Configuration bundle for the Docker MSVC 2002 provider.
#[derive(Debug, Clone)]
pub struct DockerMsvc2002Config {
    /// Docker image name (e.g. `"msvc2002-build:latest"`).
    pub image_name: String,
    /// CMake generator (e.g. `"NMake Makefiles"`).
    pub cmake_generator: String,
    /// Path to MSVC toolchain inside the container.
    pub toolchain_path: PathBuf,
    /// Override for the docker binary path (for testing).
    pub docker_binary: Option<String>,
}

/// Build provider that executes `cmkr` + `cmake` inside a Docker
/// container with the MSVC 2002 (Visual Studio 6) compiler.
pub struct DockerMsvc2002BuildProvider {
    config: DockerMsvc2002Config,
    /// Cached build directory from `configure_project`.
    build_dir: tokio::sync::Mutex<Option<PathBuf>>,
    /// Cached project root from `configure_project`.
    project_root: tokio::sync::Mutex<Option<PathBuf>>,
}

impl DockerMsvc2002BuildProvider {
    pub fn new(config: DockerMsvc2002Config) -> Self {
        Self {
            config,
            build_dir: tokio::sync::Mutex::new(None),
            project_root: tokio::sync::Mutex::new(None),
        }
    }

    fn docker_bin(&self) -> &str {
        self.config.docker_binary.as_deref().unwrap_or("docker")
    }

    /// Validate that an argument contains no shell metacharacters.
    pub(crate) fn validate_no_metacharacters(arg: &str) -> BuildResult<()> {
        const FORBIDDEN: &[char] = &[
            ';', '|', '&', '$', '`', '(', ')', '{', '}', '<', '>', '\'', '"', '\\', '\n', '\r',
        ];
        if arg.contains(FORBIDDEN) {
            return Err(BuildProviderError::ValidationRejected {
                reason: format!("shell metacharacter in argument: {arg}"),
            });
        }
        Ok(())
    }

    /// Validate that the image name matches the configured one.
    pub(crate) fn validate_image_name(&self, image: &str) -> BuildResult<()> {
        if image != self.config.image_name {
            return Err(BuildProviderError::ValidationRejected {
                reason: format!(
                    "image '{image}' is not the configured image '{}'",
                    self.config.image_name
                ),
            });
        }
        Ok(())
    }

    /// Validate that a path is contained within the project root.
    fn validate_path_containment(path: &Path, project_root: &Path) -> BuildResult<()> {
        let canonical = if path.is_absolute() {
            path.to_path_buf()
        } else {
            project_root.join(path)
        };
        if !canonical.starts_with(project_root) {
            return Err(BuildProviderError::ValidationRejected {
                reason: format!(
                    "path '{}' escapes project root '{}'",
                    canonical.display(),
                    project_root.display()
                ),
            });
        }
        Ok(())
    }

    /// Run a docker command with validated arguments and capture output.
    async fn docker_exec(
        &self,
        container_name: &str,
        args: &[&str],
    ) -> BuildResult<(String, String, i32)> {
        Self::validate_no_metacharacters(container_name)?;
        for arg in args {
            Self::validate_no_metacharacters(arg)?;
        }
        let output = Command::new(self.docker_bin())
            .arg("exec")
            .arg(container_name)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let code = output.status.code().unwrap_or(-1);
        Ok((stdout, stderr, code))
    }

    /// Run a docker run command for initial setup.
    async fn docker_run_detached(
        &self,
        container_name: &str,
        mount_source: &Path,
        mount_target: &str,
    ) -> BuildResult<()> {
        Self::validate_no_metacharacters(container_name)?;
        Self::validate_no_metacharacters(mount_target)?;
        self.validate_image_name(&self.config.image_name.clone())?;
        let output = Command::new(self.docker_bin())
            .args([
                "run",
                "-d",
                "--name",
                container_name,
                "-v",
                &format!("{}:{mount_target}", mount_source.display()),
                "-w",
                mount_target,
                &self.config.image_name,
                "sleep",
                "infinity",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(BuildProviderError::DockerUnreachable(stderr.into_owned()));
        }
        Ok(())
    }
}

/// Derive a deterministic container name from the project root.
fn container_name(project_root: &Path) -> String {
    let hash = blake3::hash(project_root.to_string_lossy().as_bytes());
    format!("autore-build-{}", &hash.to_hex()[..12])
}

#[async_trait]
impl BuildProviderTrait for DockerMsvc2002BuildProvider {
    async fn configure_project(
        &self,
        generator_manifest: &GeneratorManifest,
        project_root: &Path,
    ) -> BuildResult<BuildConfigured> {
        Self::validate_path_containment(&generator_manifest.project_root, project_root)?;
        let cname = container_name(project_root);
        let mount_target = "/workspace";
        // Start a long-running container with the project mounted.
        self.docker_run_detached(&cname, project_root, mount_target)
            .await?;
        // Run `cmkr gen` inside the container.
        let (stdout, stderr, code) = self.docker_exec(&cname, &["cmkr", "gen"]).await?;
        let success = code == 0;
        let build_dir = PathBuf::from(mount_target).join("build");
        *self.build_dir.lock().await = Some(build_dir.clone());
        *self.project_root.lock().await = Some(project_root.to_path_buf());
        Ok(BuildConfigured {
            build_dir,
            success,
            stdout,
            stderr,
        })
    }

    async fn compile_units(&self, units: &[CompileUnit]) -> BuildResult<CompileResult> {
        let cname_guard = self.project_root.lock().await;
        let project_root = cname_guard
            .as_ref()
            .ok_or_else(|| BuildProviderError::Other("project not configured".into()))?;
        let cname = container_name(project_root);
        drop(cname_guard);
        // Validate each unit path.
        let root = self.project_root.lock().await;
        let root = root.as_ref().unwrap();
        for unit in units {
            Self::validate_path_containment(&unit.source_path, root)?;
        }
        // Run cmake --build for the whole project (cmake handles per-unit deps).
        let (stdout, stderr, code) = self
            .docker_exec(&cname, &["cmake", "--build", "build"])
            .await?;
        let success = code == 0;
        let objects: Vec<PathBuf> = units.iter().map(|u| u.object_path.clone()).collect();
        Ok(CompileResult {
            objects,
            success,
            stdout,
            stderr,
        })
    }

    async fn link_target(&self, target_artifacts: &[PathBuf]) -> BuildResult<LinkResult> {
        let guard = self.project_root.lock().await;
        let project_root = guard
            .as_ref()
            .ok_or_else(|| BuildProviderError::Other("project not configured".into()))?;
        let cname = container_name(project_root);
        let build_dir = self.build_dir.lock().await;
        let build_dir = build_dir
            .as_ref()
            .ok_or_else(|| BuildProviderError::Other("build dir not set".into()))?
            .clone();
        drop(guard);
        drop(build_dir);
        // cmake --build <build-dir> --target <exe-target>
        let (stdout, stderr, code) = self
            .docker_exec(&cname, &["cmake", "--build", "build", "--target", "all"])
            .await?;
        let success = code == 0;
        let executable = target_artifacts
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("build/output.exe"));
        Ok(LinkResult {
            executable,
            success,
            stdout,
            stderr,
        })
    }

    async fn run_test(&self, test_target: &str) -> BuildResult<RunTestResult> {
        Self::validate_no_metacharacters(test_target)?;
        let guard = self.project_root.lock().await;
        let project_root = guard
            .as_ref()
            .ok_or_else(|| BuildProviderError::Other("project not configured".into()))?;
        let cname = container_name(project_root);
        drop(guard);
        let (stdout, stderr, code) = self.docker_exec(&cname, &[test_target]).await?;
        Ok(RunTestResult {
            exit_code: code,
            stdout,
            stderr,
        })
    }

    async fn collect_diagnostics(
        &self,
        build_logs: &BuildLogs,
    ) -> BuildResult<Vec<BuildDiagnostic>> {
        Ok(parse_msvc_diagnostics(&build_logs.stderr))
    }
}
