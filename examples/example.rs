use header_proxy_gateway;
use hyper::client::HttpConnector;
use hyper_tls::HttpsConnector;
use std::future::Future;
use std::pin::Pin;
#[tokio::main]
async fn main() {
    let config = header_proxy_gateway::Config {
        addr: ([127, 0, 0, 1], 3000).into(),
        auth: |_redirect: &header_proxy_gateway::ConfigRedirect,
               _headers: &hyper::HeaderMap,
               _hyper_client: &hyper::Client<HttpsConnector<HttpConnector>>,
               _hyer_builder: &hyper::http::request::Builder|
         -> Pin<Box<dyn Future<Output = (bool, String)> + Send>> {
            let future = async move {
                return (true, String::from(""));
            };

            Box::pin(future)
        },
        clear_cache_interval_in_seconds: 5 * 60,
        redirects: vec![(
            "example".to_string(),
            header_proxy_gateway::ConfigRedirect {
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

    header_proxy_gateway::run_gateway(config).await;
}
