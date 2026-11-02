//! A fake S3 server for the integration tests, and helpers to run aws3.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as ConnBuilder;
use s3s::auth::SimpleAuth;
use s3s::service::S3ServiceBuilder;
use s3s_fs::FileSystem;
use tempfile::TempDir;

const ACCESS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";
const SECRET_KEY: &str = "wJalrXUtnFI/K7MDENG/bPxRfiCYEXAMPLEKEY";

/// A fake S3 server storing the buckets in a temporary directory, and a
/// temporary working directory to run aws3 in.
pub struct Env {
    endpoint: String,
    storage: TempDir,
    work: TempDir,
    _runtime: tokio::runtime::Runtime,
}

impl Env {
    pub fn new() -> Self {
        let storage = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let service = {
            let mut builder = S3ServiceBuilder::new(FileSystem::new(storage.path()).unwrap());
            builder.set_auth(SimpleAuth::from_single(ACCESS_KEY, SECRET_KEY));
            builder.build()
        };
        runtime.spawn(async move {
            let server = ConnBuilder::new(TokioExecutor::new());
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    continue;
                };
                let conn = server
                    .serve_connection(TokioIo::new(socket), service.clone())
                    .into_owned();
                tokio::spawn(conn);
            }
        });
        Self {
            endpoint,
            storage,
            work,
            _runtime: runtime,
        }
    }

    /// Returns the working directory.
    pub fn work(&self) -> &Path {
        self.work.path()
    }

    /// Returns the path in the working directory.
    pub fn path(&self, path: &str) -> PathBuf {
        self.work.path().join(path)
    }

    /// Creates the file in the working directory, with the parents.
    pub fn write(&self, path: &str, content: impl AsRef<[u8]>) {
        let path = self.path(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    pub fn read(&self, path: &str) -> Vec<u8> {
        std::fs::read(self.path(path)).unwrap()
    }

    /// Creates the bucket in the storage of the server.
    pub fn bucket(&self, bucket: &str) {
        std::fs::create_dir_all(self.storage.path().join(bucket)).unwrap();
    }

    /// Creates the object in the storage of the server.
    pub fn object(&self, bucket: &str, key: &str, content: impl AsRef<[u8]>) {
        let path = self.storage.path().join(bucket).join(key);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Returns the HeadObject response of the object, to check the metadata.
    pub fn head(
        &self,
        bucket: &str,
        key: &str,
    ) -> aws_sdk_s3::operation::head_object::HeadObjectOutput {
        let config = aws_sdk_s3::Config::builder()
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .region(aws_sdk_s3::config::Region::new("us-east-1"))
            .endpoint_url(&self.endpoint)
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                ACCESS_KEY, SECRET_KEY, None, None, "test",
            ))
            .force_path_style(true)
            .build();
        let client = aws_sdk_s3::Client::from_conf(config);
        self._runtime
            .block_on(client.head_object().bucket(bucket).key(key).send())
            .unwrap()
    }

    /// Returns the endpoint of the server.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Returns the content of the object in the storage of the server.
    pub fn object_content(&self, bucket: &str, key: &str) -> Option<Vec<u8>> {
        std::fs::read(self.storage.path().join(bucket).join(key)).ok()
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aws3"));
        let missing = self.work.path().join(".aws-missing");
        command
            .args(args)
            .current_dir(self.work.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", self.work.path())
            .env("AWS_ACCESS_KEY_ID", ACCESS_KEY)
            .env("AWS_SECRET_ACCESS_KEY", SECRET_KEY)
            .env("AWS_REGION", "us-east-1")
            .env("AWS_ENDPOINT_URL", &self.endpoint)
            .env("AWS_CONFIG_FILE", &missing)
            .env("AWS_SHARED_CREDENTIALS_FILE", &missing)
            .env("AWS_EC2_METADATA_DISABLED", "true")
            .stdin(Stdio::null());
        // Keep the profile path of the coverage measurement.
        if let Some(path) = std::env::var_os("LLVM_PROFILE_FILE") {
            command.env("LLVM_PROFILE_FILE", path);
        }
        command
    }

    /// Runs aws3 with the arguments.
    pub fn run(&self, args: &[&str]) -> Result {
        Result::new(self.command(args).output().unwrap())
    }

    /// Runs aws3 with the arguments and the additional environment variables.
    pub fn run_with_env(&self, args: &[&str], envs: &[(&str, &str)]) -> Result {
        let mut command = self.command(args);
        command.envs(envs.iter().copied());
        Result::new(command.output().unwrap())
    }

    /// Writes the config file with small multipart thresholds, and returns
    /// the environment variables to use it.
    pub fn multipart_config(&self) -> [(&'static str, String); 1] {
        let path = self.config(
            "[default]\ns3 =\n  multipart_threshold = 5MB\n  multipart_chunksize = 5MB\n  max_concurrent_requests = 4\n",
        );
        [("AWS_CONFIG_FILE", path.to_string_lossy().into_owned())]
    }

    /// Runs aws3 with the arguments and the standard input.
    pub fn run_with_stdin(&self, args: &[&str], stdin: &[u8]) -> Result {
        use std::io::Write;
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        Result::new(child.wait_with_output().unwrap())
    }

    /// Writes the config file of the transfer settings.
    pub fn config(&self, content: &str) -> PathBuf {
        let path = self.work.path().join(".aws-config");
        std::fs::write(&path, content).unwrap();
        path
    }
}

/// The result of running aws3.
pub struct Result {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub stdout_bytes: Vec<u8>,
}

impl Result {
    fn new(output: Output) -> Self {
        Self {
            code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            stdout_bytes: output.stdout,
        }
    }

    /// Returns the lines of the standard output in order.
    pub fn lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }

    /// Returns the sorted lines of the standard output, since the concurrent
    /// transfers complete in any order.
    pub fn sorted_lines(&self) -> Vec<&str> {
        let mut lines = self.lines();
        lines.sort();
        lines
    }

    /// Asserts the command succeeded, and returns itself.
    #[track_caller]
    pub fn success(self) -> Self {
        assert_eq!(
            self.code, 0,
            "stdout: {}\nstderr: {}",
            self.stdout, self.stderr
        );
        self
    }
}

/// Returns the content of the size, which is not repetitive in short ranges.
pub fn content(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 7 + i / 251) as u8).collect()
}
