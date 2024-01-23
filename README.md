# header_proxy_gateway

A very fast and efficient gateway proxying and caching requests to other servers based on header attributes.
It was initially built to provide a single endpoint for authentication and access to multiple proxied GraphQL servers, but can be used as proxy to any http API.


## Example
For each request the `x-server` header has to be provided.<br>
Based on `x-server` the gateway knows to which server the proxied request has to be sent.<br> `x-server` should be equaivalent to the key in the hashmap here: **"example"**.<br>

```rust
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
```

## Auth
The `auth` function receives the server to which the request is sent/redirected, the request headers, a `&reqwest_middleware::ClientWithMiddleware` supporting `cache-control` header to make requests and a `reqwest_middleware::RequestBuilder` to modify the actual request to the server based on the authentication.<br>
The latter thus supports adding additional headers during authentication e.g. a user id based on details provided or fetched during the authentication.<br><br>
The auth function returns two values, the first boolean indicates if the authentication was successful, the second is a `reqwest_middleware::RequestBuilder` which is used to construct the request to the proxied server.<br>
If you return a different `reqwest_middleware::RequestBuilder` than provided in the parameters, the request may go to a different server as set in the config.
