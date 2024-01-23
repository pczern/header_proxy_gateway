use async_trait::async_trait;
use header_proxy_gateway;
use hyper::HeaderMap;
pub struct CustomAuth;

#[async_trait]
impl header_proxy_gateway::Auth for CustomAuth {
    async fn authenticate(
        &self,
        _redirect: &header_proxy_gateway::ConfigRedirect,
        _headers: &HeaderMap,
        _client: &reqwest_middleware::ClientWithMiddleware,
        builder: reqwest_middleware::RequestBuilder,
    ) -> (bool, reqwest_middleware::RequestBuilder) {
        let builder = builder.header("x-header-1", "hello world");
        return (true, builder);
    }
}

#[tokio::main]
async fn main() {
    let config = header_proxy_gateway::Config {
        addr: ([127, 0, 0, 1], 3000).into(),
        auth: Box::new(CustomAuth {}),
        clear_cache_interval_in_seconds: 5 * 60,
        redirects: vec![(
            "example".to_string(),
            header_proxy_gateway::ConfigRedirect {
                method: reqwest::Method::POST,
                headers: vec![
                    ("x-header-1".to_string(), "hello world".to_string()),
                    ("x-header-2".to_string(), "hello world".to_string()),
                ]
                .into_iter()
                .collect(),
                url: String::from("https://example.com/graphql"),
            },
        )]
        .into_iter()
        .collect(),
    };

    match header_proxy_gateway::run(config).await {
        Ok(_) => println!("Server started"),
        Err(e) => println!("Error starting server: {}", e),
    };
}
