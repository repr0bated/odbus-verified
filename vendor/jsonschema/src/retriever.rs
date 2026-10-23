//! Logic for retrieving external resources.
use referencing::{Retrieve, Uri};
use serde_json::Value;

pub(crate) struct DefaultRetriever;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "resolve-file", test)))]
fn file_uri_path(
    uri: &Uri<String>,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>> {
    // A remote file authority must not silently address this machine's files.
    if let Some(authority) = uri.authority() {
        let authority = authority.as_str();
        if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
            return Err("Only local file URI authorities are supported".into());
        }
    }
    let path = uri.path().as_str();
    if !path.starts_with('/') {
        return Err("A file URI requires an absolute path".into());
    }
    let decoded = percent_encoding::percent_decode_str(path);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(std::ffi::OsString::from_vec(decoded.collect()).into())
    }
    #[cfg(not(unix))]
    {
        let path = decoded.decode_utf8()?;
        #[cfg(windows)]
        {
            // Standard local Windows file URIs contain an absolute drive path.
            let drive_path = path.strip_prefix('/').unwrap_or(&path);
            let bytes = drive_path.as_bytes();
            if bytes.len() < 3
                || !bytes[0].is_ascii_alphabetic()
                || bytes[1] != b':'
                || bytes[2] != b'/'
            {
                return Err("A local Windows file URI requires an absolute drive path".into());
            }
            Ok(std::path::PathBuf::from(drive_path.replace('/', "\\")))
        }
        #[cfg(not(windows))]
        {
            Ok(std::path::PathBuf::from(path.as_ref()))
        }
    }
}

impl Retrieve for DefaultRetriever {
    #[allow(unused)]
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_arch = "wasm32")]
        {
            Err("External references are not supported in WASM".into())
        }
        #[cfg(not(target_arch = "wasm32"))]
        match uri.scheme().as_str() {
            "http" | "https" => {
                #[cfg(any(feature = "resolve-http", test))]
                {
                    Ok(reqwest::blocking::get(uri.as_str())?.json()?)
                }
                #[cfg(not(any(feature = "resolve-http", test)))]
                Err("`resolve-http` feature or a custom resolver is required to resolve external schemas via HTTP".into())
            }
            "file" => {
                #[cfg(any(feature = "resolve-file", test))]
                {
                    let path = file_uri_path(uri)?;
                    let file = std::fs::File::open(path)?;
                    Ok(serde_json::from_reader(file)?)
                }
                #[cfg(not(any(feature = "resolve-file", test)))]
                {
                    Err("`resolve-file` feature or a custom resolver is required to resolve external schemas via files".into())
                }
            }
            scheme => Err(format!("Unknown scheme {scheme}").into()),
        }
    }
}

#[cfg(feature = "resolve-async")]
#[async_trait::async_trait]
impl referencing::AsyncRetrieve for DefaultRetriever {
    async fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(target_arch = "wasm32")]
        {
            Err("External references are not supported in WASM".into())
        }
        #[cfg(not(target_arch = "wasm32"))]
        match uri.scheme().as_str() {
            "http" | "https" => {
                #[cfg(any(feature = "resolve-http", test))]
                {
                    Ok(reqwest::get(uri.as_str()).await?.json().await?)
                }
                #[cfg(not(any(feature = "resolve-http", test)))]
                Err("`resolve-http` feature or a custom resolver is required to resolve external schemas via HTTP".into())
            }
            "file" => {
                #[cfg(any(feature = "resolve-file", test))]
                {
                    // File operations are blocking, so we use tokio's spawn_blocking
                    let path = file_uri_path(uri)?;
                    let contents = tokio::task::spawn_blocking(
                        move || -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
                            let file = std::fs::File::open(path)?;
                            Ok(serde_json::from_reader(file)?)
                        },
                    )
                    .await??;
                    Ok(contents)
                }
                #[cfg(not(any(feature = "resolve-file", test)))]
                {
                    Err("`resolve-file` feature or a custom resolver is required to resolve external schemas via files".into())
                }
            }
            scheme => Err(format!("Unknown scheme {scheme}").into()),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn path_to_uri(path: &std::path::Path) -> String {
    use percent_encoding::{percent_encode, AsciiSet, CONTROLS};

    let mut result = "file://".to_owned();
    const SEGMENT: &AsciiSet = &CONTROLS
        .add(b' ')
        .add(b'"')
        .add(b'<')
        .add(b'>')
        .add(b'`')
        .add(b'#')
        .add(b'?')
        .add(b'{')
        .add(b'}')
        .add(b'/')
        .add(b'%');

    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::ffi::OsStrExt;

        const CUSTOM_SEGMENT: &AsciiSet = &SEGMENT.add(b'\\');
        for component in path.components().skip(1) {
            result.push('/');
            result.extend(percent_encode(
                component.as_os_str().as_bytes(),
                CUSTOM_SEGMENT,
            ));
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();

        match components.next() {
            Some(Component::Prefix(ref p)) => match p.kind() {
                Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                    result.push('/');
                    result.push(letter as char);
                    result.push(':');
                }
                _ => panic!("Unexpected path"),
            },
            _ => panic!("Unexpected path"),
        }

        for component in components {
            if component == Component::RootDir {
                continue;
            }

            let component = component.as_os_str().to_str().expect("Unexpected path");

            result.push('/');
            result.extend(percent_encode(component.as_bytes(), SEGMENT));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    use super::path_to_uri;
    use serde_json::json;
    #[cfg(not(target_arch = "wasm32"))]
    use std::io::Write;

    #[test]
    #[cfg(unix)]
    fn file_uri_decodes_path_bytes_once() {
        use std::os::unix::ffi::OsStrExt;
        let uri = referencing::Uri::parse(
            "file://localhost/tmp/my%20schema%2520%23%FF.json".to_owned(),
        )
        .expect("Valid URI");
        let path = super::file_uri_path(&uri).expect("Valid local path");
        assert_eq!(path.as_os_str().as_bytes(), b"/tmp/my schema%20#\xFF.json");
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn file_uri_rejects_remote_authority() {
        let uri = referencing::Uri::parse("file://remote.example/tmp/schema.json".to_owned())
            .expect("Valid URI");
        assert!(super::file_uri_path(&uri).is_err());
    }

    #[test]
    #[cfg(windows)]
    fn file_uri_decodes_drive_path() {
        let uri = referencing::Uri::parse("file:///C:/my%20schema.json".to_owned())
            .expect("Valid URI");
        let path = super::file_uri_path(&uri).expect("Valid local path");
        assert_eq!(path, std::path::PathBuf::from(r"C:\my schema.json"));
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn test_retrieve_from_file() {
        let mut temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let external_schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });
        write!(temp_file, "{}", external_schema).expect("Failed to write to temp file");

        let uri = path_to_uri(temp_file.path());

        let schema = json!({
            "type": "object",
            "properties": {
                "user": { "$ref": uri }
            }
        });

        let validator = crate::validator_for(&schema).expect("Schema compilation failed");

        let valid = json!({"user": {"name": "John Doe"}});
        assert!(validator.is_valid(&valid));

        let invalid = json!({"user": {}});
        assert!(!validator.is_valid(&invalid));
    }

    #[test]
    fn test_unknown_scheme() {
        let schema = json!({
            "type": "object",
            "properties": {
                "test": { "$ref": "unknown-schema://test" }
            }
        });

        let result = crate::validator_for(&schema);

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        #[cfg(not(target_arch = "wasm32"))]
        assert!(error.contains("Unknown scheme"));
        #[cfg(target_arch = "wasm32")]
        assert!(error.contains("External references are not supported in WASM"));
    }
}

#[cfg(all(test, feature = "resolve-async", not(target_arch = "wasm32")))]
mod async_tests {
    use super::*;
    use crate::Registry;
    use serde_json::json;
    use std::io::Write;

    #[tokio::test]
    async fn test_async_retrieve_from_file() {
        let mut temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
        let external_schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        });
        write!(temp_file, "{}", external_schema).expect("Failed to write to temp file");

        let uri = path_to_uri(temp_file.path());

        let schema = json!({
            "type": "object",
            "properties": {
                "user": { "$ref": uri }
            }
        });

        // Create registry with default async retriever
        let registry = Registry::options()
            .async_retriever(DefaultRetriever)
            .build([(
                "http://example.com/schema",
                crate::Draft::Draft202012.create_resource(schema.clone()),
            )])
            .await
            .expect("Registry creation failed");

        let validator = crate::options()
            .with_registry(registry)
            .build(&schema)
            .expect("Invalid schema");

        let valid = json!({"user": {"name": "John Doe"}});
        assert!(validator.is_valid(&valid));

        let invalid = json!({"user": {}});
        assert!(!validator.is_valid(&invalid));
    }

    #[tokio::test]
    async fn test_async_unknown_scheme() {
        let schema = json!({
            "type": "object",
            "properties": {
                "test": { "$ref": "unknown-schema://test" }
            }
        });

        let result = Registry::options()
            .async_retriever(DefaultRetriever)
            .build([(
                "http://example.com/schema",
                crate::Draft::Draft202012.create_resource(schema),
            )])
            .await;

        assert!(result.is_err());
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Unknown scheme"));
    }

    #[tokio::test]
    async fn test_async_concurrent_retrievals() {
        let mut temp_files = vec![];
        let mut uris = vec![];

        // Create multiple temp files with different schemas
        for i in 0..3 {
            let mut temp_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
            let schema = json!({
                "type": "object",
                "properties": {
                    "field": { "type": "string", "minLength": i }
                }
            });
            write!(temp_file, "{}", schema).expect("Failed to write to temp file");
            uris.push(path_to_uri(temp_file.path()));
            temp_files.push(temp_file);
        }

        // Create a schema that references all temp files
        let schema = json!({
            "type": "object",
            "properties": {
                "obj1": { "$ref": uris[0] },
                "obj2": { "$ref": uris[1] },
                "obj3": { "$ref": uris[2] }
            }
        });

        let registry = Registry::options()
            .async_retriever(DefaultRetriever)
            .build([(
                "http://example.com/schema",
                crate::Draft::Draft202012.create_resource(schema.clone()),
            )])
            .await
            .expect("Registry creation failed");

        let validator = crate::options()
            .with_registry(registry)
            .build(&schema)
            .expect("Invalid schema");

        let valid = json!({
            "obj1": { "field": "" },      // minLength: 0
            "obj2": { "field": "a" },     // minLength: 1
            "obj3": { "field": "ab" }     // minLength: 2
        });
        assert!(validator.is_valid(&valid));

        // Test invalid data
        let invalid = json!({
            "obj1": { "field": "" },
            "obj2": { "field": "" },      // should be at least 1 char
            "obj3": { "field": "a" }      // should be at least 2 chars
        });
        assert!(!validator.is_valid(&invalid));
    }
}
