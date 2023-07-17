use pyo3::{PyResult, Python};
use serde::{Deserialize, Serialize};


#[derive(Serialize, Deserialize, Debug)]
pub struct HTTPScope {
    #[serde(rename(serialize = "type", deserialize = "type"))]
    pub tp: String,
    pub version: String,
    pub spec_version: String,
    pub http_version: String,
    pub method: String,
    pub scheme: String,
    pub path: String,
    pub raw_path: Vec<u8>,
    pub query_string: String,
    pub root_path: Option<String>,
    pub headers: Vec<(Vec<u8>, Vec<u8>)>,
    pub client: Option<(String, usize)>,
    pub server: (String, u16),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AsgiHTTPReceiveEvent {
    scope: HTTPScope,
    tp: String,
    body: Vec<u8>,
    more_body: bool,
}

