//! reqwest-backed `HttpClient` HAL — the only outbound path, used for the cloud LLM.

use std::future::Future;

use tar_hal::{HalError, HalResult, HttpClient, HttpResponse};

pub struct LinuxHttp {
    client: reqwest::Client,
}

impl LinuxHttp {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
    }
}

impl Default for LinuxHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient for LinuxHttp {
    fn post(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> impl Future<Output = HalResult<HttpResponse>> {
        // Own the inputs so the returned future borrows nothing.
        let client = self.client.clone();
        let url = url.to_string();
        let headers: Vec<(String, String)> =
            headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let body = body.to_vec();
        async move {
            let mut req = client.post(&url).body(body);
            for (k, v) in &headers {
                req = req.header(k, v);
            }
            let resp = req.send().await.map_err(|e| HalError::Io(e.to_string()))?;
            let status = resp.status().as_u16();
            let bytes = resp.bytes().await.map_err(|e| HalError::Io(e.to_string()))?;
            Ok(HttpResponse { status, body: bytes.to_vec() })
        }
    }
}
