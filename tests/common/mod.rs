//! A fake S3 server for the integration tests, and helpers to run aws3.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};

use http_body_util::BodyExt;
use hyper::body::Incoming;
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
    requests: Arc<Mutex<Vec<Request>>>,
    regions: Arc<Mutex<HashMap<String, String>>>,
    _runtime: tokio::runtime::Runtime,
}

/// The request received by the server, to check the headers.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: http::Method,
    pub uri: http::Uri,
    pub headers: http::HeaderMap,
}

impl Request {
    /// Returns the bucket of the path-style request.
    pub fn bucket(&self) -> &str {
        let path = self.uri.path().trim_start_matches('/');
        path.split('/').next().unwrap_or_default()
    }

    /// Returns the region in the credential scope of the signature.
    pub fn signed_region(&self) -> &str {
        let authorization = self.headers.get("authorization");
        let authorization = authorization.and_then(|value| value.to_str().ok());
        let credential = authorization.and_then(|value| value.split("Credential=").nth(1));
        credential
            .and_then(|value| value.split('/').nth(2))
            .unwrap_or_default()
    }
}

impl Env {
    pub fn new() -> Self {
        Self::start(false)
    }

    /// Starts the server with TLS, whose certificate is signed by a CA in
    /// `ca.pem` of the working directory.
    pub fn new_tls() -> Self {
        Self::start(true)
    }

    fn start(tls: bool) -> Self {
        let storage = TempDir::new().unwrap();
        let work = TempDir::new().unwrap();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let scheme = if tls { "https" } else { "http" };
        let endpoint = format!("{scheme}://{}", listener.local_addr().unwrap());
        let acceptor =
            tls.then(|| tls_acceptor(&work.path().join("ca.pem"), &[b"h2", b"http/1.1"]));
        let s3 = {
            let mut builder = S3ServiceBuilder::new(FileSystem::new(storage.path()).unwrap());
            builder.set_auth(SimpleAuth::from_single(ACCESS_KEY, SECRET_KEY));
            builder.build()
        };
        // The request bodies are read before the requests are handled, since
        // the server may respond early, e.g. on precondition failures, and
        // close the connection while the client is writing the body.
        let requests = Arc::new(Mutex::new(Vec::new()));
        let regions = Arc::new(Mutex::new(HashMap::<String, String>::new()));
        let service = hyper::service::service_fn({
            let (requests, regions) = (requests.clone(), regions.clone());
            move |request: http::Request<Incoming>| {
                let (s3, requests, regions) = (s3.clone(), requests.clone(), regions.clone());
                async move {
                    let (parts, body) = request.into_parts();
                    let body = body.collect().await.map_err(|err| err.to_string())?;
                    let request = Request {
                        method: parts.method.clone(),
                        uri: parts.uri.clone(),
                        headers: parts.headers.clone(),
                    };
                    requests.lock().unwrap().push(request.clone());
                    // The requests to the bucket in another region are
                    // redirected like S3.
                    let region = regions.lock().unwrap().get(request.bucket()).cloned();
                    if let Some(region) = region
                        && request.signed_region() != region
                    {
                        return Ok(redirect_response(&request, &region));
                    }
                    let encode = parts.uri.query().is_some_and(|query| {
                        query.split('&').any(|param| param == "encoding-type=url")
                    });
                    let request =
                        http::Request::from_parts(parts, s3s::Body::from(body.to_bytes()));
                    let mut response = s3.call(request).await.map_err(|err| format!("{err:?}"))?;
                    // The checksums of CRC64NVME are of the full objects on S3,
                    // whose type is not responded by the fake server.
                    let headers = response.headers_mut();
                    if headers.contains_key("x-amz-checksum-crc64nvme")
                        && !headers.contains_key("x-amz-checksum-type")
                    {
                        let value = http::HeaderValue::from_static("FULL_OBJECT");
                        headers.insert("x-amz-checksum-type", value);
                    }
                    if !encode || !response.status().is_success() {
                        return Ok(response);
                    }
                    let (mut parts, body) = response.into_parts();
                    let body = body.collect().await.map_err(|err| err.to_string())?;
                    let body = encode_keys(&String::from_utf8_lossy(&body.to_bytes()));
                    parts.headers.remove(http::header::CONTENT_LENGTH);
                    Ok::<_, String>(http::Response::from_parts(parts, s3s::Body::from(body)))
                }
            }
        });
        runtime.spawn(async move {
            let server = ConnBuilder::new(TokioExecutor::new());
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    continue;
                };
                let (server, service, acceptor) =
                    (server.clone(), service.clone(), acceptor.clone());
                tokio::spawn(async move {
                    match acceptor {
                        Some(acceptor) => {
                            // The handshake fails when the client rejects the certificate.
                            if let Ok(socket) = acceptor.accept(socket).await {
                                let _ =
                                    server.serve_connection(TokioIo::new(socket), service).await;
                            }
                        }
                        None => {
                            let _ = server.serve_connection(TokioIo::new(socket), service).await;
                        }
                    }
                });
            }
        });
        Self {
            endpoint,
            storage,
            work,
            requests,
            regions,
            _runtime: runtime,
        }
    }

    /// Places the bucket in the region, where the requests signed for other
    /// regions are redirected.
    pub fn bucket_region(&self, bucket: &str, region: &str) {
        let mut regions = self.regions.lock().unwrap();
        regions.insert(bucket.to_owned(), region.to_owned());
    }

    /// Returns the requests received by the server so far.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    /// Starts an HTTP proxy, which tunnels the CONNECT requests and forwards
    /// the other requests to the server of the request line.
    pub fn proxy(&self) -> Proxy {
        self.start_proxy(None)
    }

    /// Starts an HTTPS proxy, whose certificate is signed by a CA in
    /// `proxy-ca.pem` of the working directory.
    pub fn tls_proxy(&self) -> Proxy {
        let acceptor = tls_acceptor(&self.work.path().join("proxy-ca.pem"), &[b"http/1.1"]);
        self.start_proxy(Some(acceptor))
    }

    fn start_proxy(&self, acceptor: Option<tokio_rustls::TlsAcceptor>) -> Proxy {
        use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
        trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
        impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
        let scheme = if acceptor.is_some() { "https" } else { "http" };
        let listener = self
            ._runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let requests = Arc::new(Mutex::new(Vec::new()));
        self._runtime.spawn({
            let requests = requests.clone();
            async move {
                loop {
                    let Ok((socket, _)) = listener.accept().await else {
                        continue;
                    };
                    let (requests, acceptor) = (requests.clone(), acceptor.clone());
                    tokio::spawn(async move {
                        let socket: Box<dyn Stream> = match acceptor {
                            // The handshake fails when the client rejects the certificate.
                            Some(acceptor) => match acceptor.accept(socket).await {
                                Ok(socket) => Box::new(socket),
                                Err(_) => return,
                            },
                            None => Box::new(socket),
                        };
                        let mut client = BufReader::new(socket);
                        let mut head = String::new();
                        loop {
                            let mut line = String::new();
                            if client.read_line(&mut line).await.unwrap_or(0) == 0 {
                                return;
                            }
                            head.push_str(&line);
                            if line == "\r\n" {
                                break;
                            }
                        }
                        let request_line = head.lines().next().unwrap_or_default().to_owned();
                        let auth = head.lines().find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("proxy-authorization")
                                .then(|| value.trim().to_owned())
                        });
                        requests.lock().unwrap().push(ProxyRequest {
                            line: request_line.clone(),
                            auth,
                        });
                        let mut parts = request_line.split(' ');
                        let (method, target) = (parts.next().unwrap(), parts.next().unwrap());
                        let host = if method == "CONNECT" {
                            target.to_owned()
                        } else {
                            let uri: http::Uri = target.parse().unwrap();
                            uri.authority().unwrap().to_string()
                        };
                        let Ok(mut server) = tokio::net::TcpStream::connect(host).await else {
                            return;
                        };
                        if method == "CONNECT" {
                            let response = b"HTTP/1.1 200 Connection established\r\n\r\n";
                            let _ = client.get_mut().write_all(response).await;
                        } else {
                            let _ = server.write_all(head.as_bytes()).await;
                        }
                        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                    });
                }
            }
        });
        Proxy {
            scheme,
            address,
            requests,
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
            .http_client(aws_smithy_http_client::Builder::new().build_http())
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

    /// Returns the number of the ongoing multipart uploads in the storage of
    /// the server, which stores the information in the root directory.
    pub fn multipart_uploads(&self) -> usize {
        std::fs::read_dir(self.storage.path())
            .unwrap()
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|name| name.starts_with(".upload-") && name.ends_with(".json"))
            .count()
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
            r"[default]
s3 =
  multipart_threshold = 5MB
  multipart_chunksize = 5MB
  max_concurrent_requests = 4
",
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

    /// Runs aws3 with the arguments and the standard output closed by the
    /// reader, like piping to `head`.
    pub fn run_with_closed_stdout(&self, args: &[&str]) -> Result {
        let mut child = self
            .command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        Result::new(child.wait_with_output().unwrap())
    }

    /// Writes the config file of the transfer settings.
    pub fn config(&self, content: &str) -> PathBuf {
        let path = self.work.path().join(".aws-config");
        std::fs::write(&path, content).unwrap();
        path
    }
}

/// Returns the response of S3 to the request to the bucket in another region.
fn redirect_response(request: &Request, region: &str) -> http::Response<s3s::Body> {
    let body = if request.method == http::Method::HEAD {
        String::new()
    } else {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Error><Code>PermanentRedirect</Code>\
             <Message>The bucket you are attempting to access must be addressed using the \
             specified endpoint. Please send all future requests to this endpoint.</Message>\
             <Bucket>{}</Bucket></Error>",
            request.bucket()
        )
    };
    http::Response::builder()
        .status(301)
        .header("x-amz-bucket-region", region)
        .header("content-type", "application/xml")
        .body(s3s::Body::from(body))
        .unwrap()
}

/// URL-encodes the keys and the prefixes in the response of listing objects,
/// since the fake server ignores `encoding-type=url` unlike S3.
fn encode_keys(xml: &str) -> String {
    let mut encoded = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest
        .find("<Key>")
        .into_iter()
        .chain(rest.find("<Prefix>"))
        .min()
    {
        let (head, tail) = rest.split_at(start);
        let (tag, tail) = tail.split_at(tail.find('>').unwrap() + 1);
        let end = tail.find("</").unwrap();
        encoded.push_str(head);
        encoded.push_str(tag);
        let value = tail[..end]
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&");
        for byte in value.bytes() {
            match byte {
                b' ' => encoded.push('+'),
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                    encoded.push(byte as char)
                }
                _ => encoded.push_str(&format!("%{byte:02X}")),
            }
        }
        rest = &tail[end..];
    }
    encoded.push_str(rest);
    encoded
}

/// Creates the acceptor of TLS connections with a certificate signed by a new
/// CA, which is written to the path.
fn tls_acceptor(ca_path: &Path, alpn: &[&[u8]]) -> tokio_rustls::TlsAcceptor {
    use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, IsCa, KeyPair};
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let ca = CertifiedIssuer::self_signed(ca_params, KeyPair::generate().unwrap()).unwrap();
    std::fs::write(ca_path, ca.pem()).unwrap();
    let key = KeyPair::generate().unwrap();
    let params =
        CertificateParams::new(vec!["127.0.0.1".to_owned(), "localhost".to_owned()]).unwrap();
    let cert = params.signed_by(&key, &ca).unwrap();
    let provider = std::sync::Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into()),
        )
        .unwrap();
    config.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
    tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config))
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

/// Waits until the condition holds, which panics after a long time to avoid
/// hanging the tests.
#[track_caller]
pub fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Waits for the exit of the child, which is killed after a long time to avoid
/// hanging the tests.
#[track_caller]
pub fn wait_child(mut child: std::process::Child) -> Output {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            panic!("timed out waiting for the exit of aws3");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.wait_with_output().unwrap()
}

/// Returns the content of the size, which is not repetitive in short ranges.
pub fn content(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i * 7 + i / 251) as u8).collect()
}

/// An HTTP proxy started by [`Env::proxy`] or [`Env::tls_proxy`].
pub struct Proxy {
    scheme: &'static str,
    address: String,
    requests: Arc<Mutex<Vec<ProxyRequest>>>,
}

/// The first request of a connection to the proxy.
#[derive(Debug, Clone)]
pub struct ProxyRequest {
    pub line: String,
    pub auth: Option<String>,
}

impl Proxy {
    /// Returns the URL of the proxy, with the credentials if specified.
    pub fn url(&self, credentials: Option<&str>) -> String {
        match credentials {
            Some(credentials) => format!("{}://{credentials}@{}", self.scheme, self.address),
            None => format!("{}://{}", self.scheme, self.address),
        }
    }

    /// Returns the first requests of the connections so far.
    pub fn requests(&self) -> Vec<ProxyRequest> {
        self.requests.lock().unwrap().clone()
    }
}
