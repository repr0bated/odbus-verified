    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to OVSDB socket")?;

        let request = json!({
            "method": method,
            "params": params,
            "id": 0
        });

        let request_str = simd_json::to_string(&request)?;
        debug!("OVSDB request: {}", request_str);

        stream.write_all(request_str.as_bytes()).await?;
        stream.write_all(b"\n").await?;

        let mut response_buf = Vec::new();
        tokio::time::timeout(self.timeout, tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut response_buf))
            .await
            .context("OVSDB response timeout")??;

        let mut response_str = String::from_utf8(response_buf)?;
        debug!("OVSDB response: {}", response_str.trim());

        let response: Value = unsafe { simd_json::from_str(&mut response_str)? };

        if let Some(error) = response.get("error") {
            if !error.is_null() {
                return Err(anyhow::anyhow!("OVSDB error: {}", error));
            }
        }

        Ok(response["result"].clone())
    }
