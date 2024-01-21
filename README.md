# header_proxy_gateway

A very fast and efficient gateway proxying and caching requests to other servers based on header attributes.
It was built to provide a single endpoint for authentication and access to multiple proxied GraphQL servers, but can be used as proxy whenever http method `POST` is used and `content-type` is set to `application/json`.


## Example
For each request `x-server` and `x-cache-id` headers have to be provided.<br>Based on `x-cache-id` the request is cached and `x-server` is the hashmap key *here:* **example** for a redirect in the `redirects` property.<br>
Based on `x-server` the gateway knows to which server to redirect the request.

The `auth` function receives the server to which the request is sent/redirected, provides the request headers and a reference to `hyper::http::request::Builder`. Using the latter additional headers can be added to the authenticated request based on details provided or fetched during the authentication.
The auth function returns two values, the first boolean indicates if the authentication was successful, the second string represents a unique identifier for the user which ensures cached responses for authenticated users don't get shared with other users.

```rust
use header_proxy_gateway;
use std::collections::HashMap;

#[tokio::main]
async fn main() {
    let redirects: HashMap<String, header_proxy_gateway::ConfigRedirect> = vec![(
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
    .collect();

    let config = header_proxy_gateway::Config {
        addr: ([127, 0, 0, 1], 3000).into(),
        auth: |_redirect: &header_proxy_gateway::ConfigRedirect,
               _headers: &hyper::HeaderMap,
               _client: &hyper::http::request::Builder|
         -> (bool, String) {
            return (true, String::from(""));
        },
        clear_cache_interval_in_seconds: 5 * 60,
        redirects: redirects,
    };

    header_proxy_gateway::run_gateway(config).await;
}
```
