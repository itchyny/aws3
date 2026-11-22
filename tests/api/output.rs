//! The tests of the output formats, the queries and the parameter files of the
//! api command.

use crate::common::{ACCESS_KEY, Env, SECRET_KEY};

#[test]
fn query() {
    let env = Env::new();
    env.bucket("bucket");
    for (key, content) in [("a.txt", "abc"), ("b.txt", "x"), ("dir/c.txt", "hello")] {
        env.object("bucket", key, content);
    }
    let query = |args: &[&str]| {
        let mut command = vec!["api", "list-objects-v2", "--bucket", "bucket"];
        command.extend(args);
        env.run(&command)
    };

    // The query is applied to the merged pages.
    let result = query(&["--page-size", "1", "--query", "Contents[?Size > `1`].Key"]).success();
    assert_eq!(
        result.stdout,
        r#"[
    "a.txt",
    "dir/c.txt"
]
"#
    );
    let result = query(&["--query", "{n: length(Contents), s: sum(Contents[].Size)}"]).success();
    assert_eq!(
        result.stdout,
        r#"{
    "n": 3,
    "s": 9
}
"#
    );
    let result = query(&["--query", "Contents[0].Key"]).success();
    assert_eq!(result.stdout, "\"a.txt\"\n");
    let result = query(&["--query", "Missing"]).success();
    assert_eq!(result.stdout, "null\n");
    // The empty objects are not printed.
    let result = query(&["--query", "`{}`"]).success();
    assert_eq!(result.stdout, "");

    let result = env
        .run(&[
            "api",
            "head-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--query",
            "[ContentLength, ETag]",
        ])
        .success();
    assert_eq!(
        result.stdout,
        r#"[
    3,
    "\"900150983cd24fb0d6963f7d28e17f72\""
]
"#
    );

    let result = query(&["--query", "Contents["]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Bad value for --query Contents[: \
         Invalid jmespath expression: Incomplete expression:\n\"Contents[\"\n          ^\n"
    );
    // The expression is validated before sending the requests.
    assert!(
        env.requests()
            .iter()
            .all(|request| request.method != http::Method::PUT)
    );

    let result = query(&["--query", "length(`1`)"]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: In function length(), invalid type for value: 1, expected \
         string, array or object, received: \"number\"\n"
    );
}

#[test]
fn output_text() {
    let env = Env::new();
    env.bucket("bucket");
    for (key, content) in [("a.txt", "abc"), ("b.txt", "x"), ("dir/c.txt", "hello")] {
        env.object("bucket", key, content);
    }
    let run = |args: &[&str]| {
        let mut command = vec!["api"];
        command.extend(args);
        command.extend(["--output", "text"]);
        env.run(&command).success().stdout
    };

    // The scalars are written in the order of the keys, and the nested values
    // follow with the upper-cased keys.
    let stdout = run(&["list-objects-v2", "--bucket", "bucket", "--delimiter", "/"]);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines[0], "None\tNone");
    assert_eq!(lines[1], "COMMONPREFIXES\tdir/");
    assert!(lines[2].starts_with("CONTENTS\ta.txt\t20"), "{stdout}");
    assert!(lines[2].ends_with("\t3"), "{stdout}");
    assert!(lines[3].starts_with("CONTENTS\tb.txt\t"), "{stdout}");
    assert_eq!(lines.len(), 4);

    // The pages are written one by one with the query, followed by the token
    // to resume.
    let stdout = run(&[
        "list-objects-v2",
        "--bucket",
        "bucket",
        "--page-size",
        "2",
        "--max-items",
        "2",
        "--query",
        "Contents[].Key",
    ]);
    assert_eq!(stdout, "a.txt\tb.txt\nNone\n");
    let stdout = run(&["list-objects-v2", "--bucket", "bucket", "--max-items", "1"]);
    assert!(
        stdout.ends_with(
            "\nNEXTTOKEN\teyJDb250aW51YXRpb25Ub2tlbiI6IG51bGwsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9\n"
        ),
        "{stdout}"
    );

    let stdout = run(&["get-bucket-location", "--bucket", "bucket"]);
    assert_eq!(stdout, "None\n");
}

#[test]
fn output_format() {
    let env = Env::new();
    env.bucket("bucket");
    let args = ["api", "get-bucket-location", "--bucket", "bucket"];
    let json = r#"{
    "LocationConstraint": null
}
"#;

    // The format is of the option, the environment variable, or the config
    // file of the profile.
    let config = env.config(&format!(
        "[default]\noutput = text\n[profile json]\noutput = json\n\
         aws_access_key_id = {ACCESS_KEY}\naws_secret_access_key = {SECRET_KEY}\n"
    ));
    let config = config.to_str().unwrap();
    let result = env
        .run_with_env(&args, &[("AWS_CONFIG_FILE", config)])
        .success();
    assert_eq!(result.stdout, "None\n");
    let mut with_profile = args.to_vec();
    with_profile.extend(["--profile", "json"]);
    let result = env
        .run_with_env(&with_profile, &[("AWS_CONFIG_FILE", config)])
        .success();
    assert_eq!(result.stdout, json);
    let result = env
        .run_with_env(
            &args,
            &[("AWS_CONFIG_FILE", config), ("AWS_DEFAULT_OUTPUT", "json")],
        )
        .success();
    assert_eq!(result.stdout, json);
    let mut with_option = args.to_vec();
    with_option.extend(["--output", "json"]);
    let result = env
        .run_with_env(&with_option, &[("AWS_DEFAULT_OUTPUT", "text")])
        .success();
    assert_eq!(result.stdout, json);
    let mut with_off = args.to_vec();
    with_off.extend(["--output", "off"]);
    assert_eq!(env.run(&with_off).success().stdout, "");

    let result = env.run_with_env(&args, &[("AWS_DEFAULT_OUTPUT", "TEXT")]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: Unknown output type: TEXT\n"
    );
}

#[test]
fn output_table() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "abc");
    env.object("bucket", "dir/b.txt", "x");
    let run = |args: &[&str]| {
        let mut command = vec!["api"];
        command.extend(args);
        command.extend(["--output", "table"]);
        env.run(&command)
    };

    // The scalars are the columns of the rows, and the nested values are the
    // indented sections, where the title is the name of the operation.
    let result = run(&[
        "list-objects-v2",
        "--bucket",
        "bucket",
        "--delimiter",
        "/",
        "--query",
        "{Prefix: Prefix, Keys: Contents[].{Key: Key, Size: Size}, CommonPrefixes: CommonPrefixes}",
    ])
    .success();
    assert_eq!(
        result.stdout,
        "---------------------\n\
         |   ListObjectsV2   |\n\
         +----------+--------+\n\
         |  Prefix  |  None  |\n\
         +----------+--------+\n\
         || CommonPrefixes  ||\n\
         |+-----------------+|\n\
         ||     Prefix      ||\n\
         |+-----------------+|\n\
         ||  dir/           ||\n\
         |+-----------------+|\n\
         ||      Keys       ||\n\
         |+--------+--------+|\n\
         ||   Key  | Size   ||\n\
         |+--------+--------+|\n\
         ||  a.txt |  3     ||\n\
         |+--------+--------+|\n"
    );

    // The colors are reset after each write like the AWS CLI. The expected
    // outputs are of the formatter of the AWS CLI.
    let result = env
        .run(&[
            "api",
            "get-bucket-location",
            "--bucket",
            "bucket",
            "--query",
            "{a: 'x'}",
            "--output",
            "table",
            "--color",
            "on",
        ])
        .success();
    assert_eq!(
        result.stdout,
        "-------------------\n\x1b[0m\
         \x1b[0m|GetBucketLocation|\x1b[0m\x1b[0m\n\x1b[0m\
         \x1b[0m+-------+---------+\x1b[0m\x1b[0m\n\x1b[0m\
         \x1b[0m|  \x1b[1m\x1b[34ma\x1b[0m    |  \x1b[1m\x1b[34mx\x1b[0m      |\x1b[0m\x1b[0m\n\x1b[0m\
         \x1b[0m+-------+---------+\x1b[0m\x1b[0m\n\x1b[0m"
    );

    // The false values of Python are not written.
    let result = run(&[
        "get-bucket-location",
        "--bucket",
        "bucket",
        "--query",
        "LocationConstraint",
    ]);
    assert_eq!(result.success().stdout, "");

    let result = run(&[
        "get-bucket-location",
        "--bucket",
        "bucket",
        "--query",
        "`[{\"a\": 1}, \"x\"]`",
    ]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: Cannot format a list of objects mixed with other values in table\n"
    );
}

#[test]
fn output_yaml() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "abc");
    env.object("bucket", "dir/b.txt", "x");
    let run = |args: &[&str], output: &str| {
        let mut command = vec!["api"];
        command.extend(args);
        command.extend(["--output", output]);
        env.run(&command).success().stdout
    };

    // The keys of the mappings are sorted, and the strings resolved to the
    // other types are quoted.
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--query",
            "{Keys: Contents[].Key, Prefix: Prefix, Size: to_string(Contents[0].Size), Empty: `[]`}",
        ],
        "yaml",
    );
    assert_eq!(
        stdout,
        "Empty: []\nKeys:\n- a.txt\n- dir/b.txt\nPrefix: null\nSize: '3'\n"
    );
    let stdout = run(&["get-bucket-location", "--bucket", "bucket"], "yaml");
    assert_eq!(stdout, "LocationConstraint: null\n");

    // The scalars are written in JSON.
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--query",
            "Contents[0].Key",
        ],
        "yaml",
    );
    assert_eq!(stdout, "\"a.txt\"\n");

    // The pages are written one by one in the lists, where the other result
    // keys are null on the first page of the starting token.
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--page-size",
            "1",
            "--query",
            "Contents[].Key",
        ],
        "yaml-stream",
    );
    assert_eq!(stdout, "- - a.txt\n- - dir/b.txt\n");
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--starting-token",
            "eyJDb250aW51YXRpb25Ub2tlbiI6IG51bGwsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9",
            "--query",
            "[CommonPrefixes, Contents[].Key, NextToken]",
        ],
        "yaml-stream",
    );
    assert_eq!(stdout, "- - null\n  - - dir/b.txt\n  - null\n");

    // The empty response is not written.
    let stdout = run(
        &[
            "get-bucket-location",
            "--bucket",
            "bucket",
            "--query",
            "`{}`",
        ],
        "yaml-stream",
    );
    assert_eq!(stdout, "");
}

#[test]
fn parameter_files() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "abc");
    env.write("b.txt", "hello");
    env.write("key.txt", "dir/b.txt\n");
    env.write("type.txt", "text/plain");
    env.write("one.txt", "1\n");
    env.write("time.txt", "2000-01-01T00:00:00Z\n");
    env.write("attributes.txt", "ab");

    // The values of the strings are the contents of the files including the
    // newlines.
    env.run(&[
        "api",
        "put-object",
        "--bucket",
        "bucket",
        "--key",
        "file://key.txt",
        "--body",
        "b.txt",
        "--content-type=file://type.txt",
    ])
    .success();
    assert_eq!(
        env.object_content("bucket", "dir/b.txt\n").unwrap(),
        b"hello"
    );
    assert_eq!(
        env.head("bucket", "dir/b.txt\n").content_type(),
        Some("text/plain")
    );

    // The integers and the timestamps are parsed from the contents.
    let stdout = env
        .run(&[
            "api",
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--max-keys",
            "file://one.txt",
            "--query",
            "Contents[].Key",
            "--output",
            "text",
        ])
        .success()
        .stdout;
    assert_eq!(stdout, "a.txt\n");
    env.run(&[
        "api",
        "head-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--if-modified-since",
        "file://time.txt",
    ])
    .success();
    let requests = env.requests();
    let header = |name: &str| {
        let request = requests.last().unwrap();
        request
            .headers
            .get(name)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(header("if-modified-since"), "Sat, 01 Jan 2000 00:00:00 GMT");

    // The single value of a list is split into the characters like the AWS
    // CLI.
    env.run(&[
        "api",
        "list-objects-v2",
        "--bucket",
        "bucket",
        "--optional-object-attributes",
        "file://attributes.txt",
        "--no-paginate",
    ]);
    let requests = env.requests();
    let values: Vec<_> = requests
        .last()
        .unwrap()
        .headers
        .get_all("x-amz-optional-object-attributes")
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect();
    // The values are sent in the separate lines, which are equivalent to the
    // values separated by commas.
    assert_eq!(values.join(","), "a,b");

    for (args, message) in [
        (
            ["--key", "file://missing.txt"],
            "Error parsing parameter '--key': Unable to load paramfile file://missing.txt: \
             No such file or directory (os error 2)",
        ),
        (
            ["--key", "fileb://key.txt"],
            "Parameter validation failed:\nInvalid type for parameter Key, value: \"dir/b.txt\\n\", \
             type: bytes, valid types: string",
        ),
        (
            ["--sse-customer-key-md5", "fileb://key.txt"],
            "Parameter validation failed:\nInvalid type for parameter SSECustomerKeyMD5, \
             value: \"dir/b.txt\\n\", type: bytes, valid types: string",
        ),
    ] {
        let mut command = vec!["api", "head-object", "--bucket", "bucket"];
        command.extend(args);
        if args[0] != "--key" {
            command.extend(["--key", "a.txt"]);
        }
        let result = env.run(&command);
        assert_eq!(result.code, 252, "{args:?}");
        assert_eq!(
            result.stderr,
            format!("\naws3: [ERROR]: An error occurred (ParamValidation): {message}\n"),
            "{args:?}"
        );
    }

    // The parameter files of the long integers are not loaded.
    let result = env.run(&[
        "api",
        "put-object",
        "--bucket",
        "bucket",
        "--key",
        "c.txt",
        "--content-length",
        "file://one.txt",
    ]);
    assert_eq!(result.code, 252);
    assert!(
        result.stderr.contains("file://one.txt"),
        "{}",
        result.stderr
    );
}
