use anyhow::Result;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;
use crate::types::{SearchRequest, SearchResponse};

pub const PIPE_NAME: &str = r"\\.\pipe\starsearch_pipe";

pub async fn client_request(request: &SearchRequest) -> Result<SearchResponse> {
    let mut client = ClientOptions::new().open(PIPE_NAME)?;
    
    let request_data = serde_json::to_vec(request)?;
    client.write_all(&request_data).await?;
    
    // 不要 shutdown，因为我们是请求-响应模式，直接读取
    let mut response_data = vec![0u8; 65536]; // 64KB 应该够了
    let n = client.read(&mut response_data).await?;
    
    let response = serde_json::from_slice(&response_data[..n])?;
    Ok(response)
}
