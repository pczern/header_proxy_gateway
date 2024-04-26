#![warn(clippy::pedantic)]
use async_trait::async_trait;
use bytes::Bytes;
use http_body_util::BodyExt;
use http_body_util::Full;
use http_cache_reqwest::{CACacheManager, Cache, CacheMode, HttpCache, HttpCacheOptions};
use hyper;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::HeaderMap;
use hyper_util::rt::TokioIo;
use reqwest::Client;
use reqwest_middleware::ClientBuilder;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio;
use tokio::net::TcpListener;
use tokio::time::sleep;

pub struct ConfigRedirect {
    pub url: String,
    pub headers: HashMap<String, String>,
    pub method: reqwest::Method,
}

#[async_trait]
pub trait Auth: Send + Sync {
    async fn authenticate(
        &self,
        redirect: &ConfigRedirect,
        headers: &HeaderMap,
        client: &reqwest_middleware::ClientWithMiddleware,
        builder: reqwest_middleware::RequestBuilder,
    ) -> (bool, reqwest_middleware::RequestBuilder);
}

pub struct Config {
    pub addr: SocketAddr,
    pub auth: Box<dyn Auth>,
    pub clear_cache_interval_in_seconds: u64,
    pub redirects: HashMap<String, ConfigRedirect>,
}

async fn handle_request(
    config: Arc<Config>,
    req: hyper::Request<hyper::body::Incoming>,
    client: &reqwest_middleware::ClientWithMiddleware,
) -> Result<hyper::Response<Full<Bytes>>, hyper::Response<Full<Bytes>>> {
    let headers = req.headers();
    let x_server = headers.get("x-server");

    if x_server.is_none() || x_server.unwrap().to_str().is_err() {
        return Err(hyper::Response::builder()
            .status(400)
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
            .body(Full::new(Bytes::from(
                "Bad Request: missing or malformed x-server header",
            )))
            .unwrap());
    }
    let server = x_server.unwrap().to_str().unwrap();
    if !config.redirects.contains_key(server) {
        return Err(hyper::Response::builder()
            .status(400)
              .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
            .body(Full::new(Bytes::from("Bad Request")))
            .unwrap());
    }
    let redirect = config.redirects.get(server).unwrap();
    let mut request_builder = client.request(redirect.method.clone(), redirect.url.clone());
    // Append set headers for redirect route
    for (key, value) in &redirect.headers {
        request_builder = request_builder.header(key.as_str(), value.as_str());
    }

    let (is_authorized, builder) = config
        .auth
        .authenticate(redirect, req.headers(), &client, request_builder)
        .await;

    if !is_authorized {
        return Err(hyper::Response::builder()
            .status(401)
              .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
            .body(Full::new(Bytes::from("Unauthorized")))
            .unwrap());
    }

    let body_result = req.collect().await;
    if body_result.is_err() {
        if cfg!(debug_assertions) {
            println!("Error: {}", body_result.err().unwrap());
        }
        return Err(hyper::Response::builder()
            .status(500)
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
            .body(Full::new(Bytes::from("Internal Server Error")))
            .unwrap());
    }

    let req_result = builder.body(body_result.unwrap().to_bytes()).build();
    if req_result.is_err() {
        if cfg!(debug_assertions) {
            println!("Error: {}", req_result.err().unwrap());
        }
        return Err(hyper::Response::builder()
            .status(500)
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
            .body(Full::new(Bytes::from("Internal Server Error")))
            .unwrap());
    }

    let res_result = client.execute(req_result.unwrap()).await;
    if res_result.is_err() {
        if cfg!(debug_assertions) {
            println!("Error: {}", res_result.err().unwrap());
        }
        return Err(hyper::Response::builder()
            .status(500)
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
            .body(Full::new(Bytes::from("Internal Server Error")))
            .unwrap());
    }
    let res = res_result.unwrap();
    let headers = res.headers();
    let mut res_builder = hyper::Response::builder();

    for (key, value) in headers {
        res_builder = res_builder.header(key.as_str(), value.as_bytes());
    }

    let body_bytes_result = res.bytes().await;
    if body_bytes_result.is_err() {
        if cfg!(debug_assertions) {
            println!("Error: {}", body_bytes_result.err().unwrap());
        }
        return Err(hyper::Response::builder()
            .status(500)
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
            .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
            .body(Full::new(Bytes::from("Internal Server Error")))
            .unwrap());
    }
    let body = body_bytes_result.unwrap();

    Ok(res_builder
        .status(200)
        .header(hyper::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(hyper::http::header::ACCESS_CONTROL_ALLOW_HEADERS, "*")
        .header(hyper::http::header::ACCESS_CONTROL_ALLOW_METHODS, "*")
        .body(Full::new(Bytes::from(body)))
        .unwrap())
}

pub async fn run(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        pretty_env_logger::init();
    }
    let manager = CACacheManager::default();
    let client = ClientBuilder::new(Client::new())
        .with(Cache(HttpCache {
            mode: CacheMode::Default,
            manager: manager,
            options: HttpCacheOptions::default(),
        }))
        .build();
    let arc_config = Arc::new(config);

    let arc_client = Arc::new(client);
    let listener = TcpListener::bind(&arc_config.addr).await?;
    loop {
        let arc_config_clone = Arc::clone(&arc_config);
        let arc_client_clone = Arc::clone(&arc_client);
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);
   
        let service = service_fn(move |req| {
            let arc_config_clone = Arc::clone(&arc_config_clone);
            let arc_client_clone = Arc::clone(&arc_client_clone);
            async move {
                let val =
                    match handle_request(arc_config_clone, req, arc_client_clone.as_ref()).await {
                        Ok(value) => value,
                        Err(err) => err,
                    };
                Ok::<_, Infallible>(val)
            }
        });
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                println!("Failed to serve the connection: {:?}", err);
            }
        });

        let manager = CACacheManager::default();
        let arc_config2 = Arc::clone(&arc_config);

        // Spawn a task to clear the cache every 60 seconds
        tokio::spawn(async move {
            loop {
                let arc_config = Arc::clone(&arc_config2);
                sleep(Duration::from_secs(
                    arc_config.clear_cache_interval_in_seconds,
                ))
                .await;
                let manager_clone = manager.clone();
                let _ = manager_clone.clear().await;
            }
        });
    }
}