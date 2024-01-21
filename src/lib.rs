use hyper;
use hyper::body::Bytes;
use hyper::client::HttpConnector;
use hyper::service::make_service_fn;
use hyper::service::service_fn;
use hyper::HeaderMap;
use hyper::Server;
use hyper_tls;
use hyper_tls::HttpsConnector;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio;
use tokio::sync::Mutex;

pub type Cache = Arc<Mutex<(HashMap<String, Bytes>, VecDeque<String>)>>;

pub struct ConfigRedirect {
    pub url: String,
    pub headers: HashMap<String, String>,
}
pub struct Config {
    pub addr: SocketAddr,
    pub auth: fn(
        &ConfigRedirect,
        &HeaderMap,
        &hyper::Client<HttpsConnector<HttpConnector>>,
        &hyper::http::request::Builder,
    ) -> (bool, String),
    pub clear_cache_interval_in_seconds: u64,
    pub redirects: HashMap<String, ConfigRedirect>,
}

async fn handle_request(
    config: Arc<Config>,
    req: hyper::Request<hyper::Body>,
    cache: Cache,
    client: hyper::Client<HttpsConnector<HttpConnector>>,
) -> Result<hyper::Response<hyper::Body>, hyper::Response<hyper::Body>> {
    let headers = req.headers();
    let x_server = headers.get("x-server");
    let x_cache_id = headers.get("x-cache-id");

    if x_server.is_none() || x_cache_id.is_none() || x_cache_id.unwrap().len() > 50 {
        return Err(hyper::Response::builder()
            .status(400)
            .body(hyper::Body::from("Bad Request: missing correct headers"))
            .unwrap());
    }

    let server = match x_server.unwrap().to_str() {
        Ok(value) => value,
        Err(_) => {
            return Err(hyper::Response::builder()
                .status(400)
                .body(hyper::Body::from("Bad Request: missing x-server header"))
                .unwrap())
        }
    };
    if !config.redirects.contains_key(server) {
        return Err(hyper::Response::builder()
            .status(400)
            .body(hyper::Body::from("Bad Request"))
            .unwrap());
    }

    let mut request_builder = hyper::Request::builder();
    let redirect = config.redirects.get(server).unwrap();
    for (key, value) in &redirect.headers {
        request_builder = request_builder.header(key.as_str(), value.as_str());
    }

    let (is_authorized, auth_identifier) =
        (config.auth)(redirect, req.headers(), &client, &request_builder);

    if !is_authorized {
        return Err(hyper::Response::builder()
            .status(401)
            .body(hyper::Body::from("Unauthorized"))
            .unwrap());
    }

    let cache_id = match x_cache_id.unwrap().to_str() {
        Ok(value) => value,
        Err(_) => {
            return Err(hyper::Response::builder()
                .status(400)
                .body(hyper::Body::from("Bad Request: missing x-cache-id header"))
                .unwrap())
        }
    };
    let cache_identifier = auth_identifier + cache_id;
    {
        // release lock on mutex after code block
        let cache_mutex_guard = cache.lock().await;
        if let Some(cached_value) = cache_mutex_guard.0.get(&cache_identifier) {
            return Ok(hyper::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(hyper::Body::from(cached_value.clone()))
                .unwrap());
        }
    }

    let req = request_builder
        .method(hyper::Method::POST)
        .uri(redirect.url.clone())
        .header("Content-Type", "application/json")
        .body(req.into_body())
        .unwrap();

    return match client.request(req).await {
        Ok(res) => {
            if res.headers().get("content-type") == None
                || res.headers().get("content-type").unwrap().to_str().is_err()
                || !res
                    .headers()
                    .get("content-type")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("application/json")
            {
                if cfg!(debug_assertions) {
                    println!("Error: content-type");
                }
                return Err(hyper::Response::builder()
                    .status(500)
                    .body(hyper::Body::from("Internal Server Error"))
                    .unwrap());
            }

            let body = res.into_body();
            let bytes: Bytes = match hyper::body::to_bytes(body).await {
                Ok(body_bytes) => body_bytes,
                Err(e) => {
                    if cfg!(debug_assertions) {
                        println!("Error: {}", e);
                    }
                    return Err(hyper::Response::builder()
                        .status(500)
                        .body(hyper::Body::from("Internal Server Error"))
                        .unwrap());
                }
            };
            let arc_bytes: Arc<Bytes> = Arc::new(bytes);
            let arc_bytes2: Arc<Bytes> = Arc::clone(&arc_bytes);
            tokio::spawn(async move {
                let mut cache = cache.lock().await;
                if cache.1.len() > 100 {
                    if let Some(old_hash) = cache.1.pop_front() {
                        cache.0.remove(&old_hash);
                    }
                }
                cache
                    .0
                    .insert(cache_identifier.clone(), arc_bytes.as_ref().clone());
                cache.1.push_back(cache_identifier);
            });

            Ok(hyper::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(hyper::Body::from(arc_bytes2.as_ref().clone()))
                .unwrap())
        }
        Err(e) => {
            if cfg!(debug_assertions) {
                println!("Error: {}", e);
            }

            Err(hyper::Response::builder()
                .status(500)
                .body(hyper::Body::from("Internal Server Error"))
                .unwrap())
        }
    };
}

pub async fn run_gateway(config: Config) {
    let glob_cache = Arc::new(Mutex::new((HashMap::new(), VecDeque::new())));
    let glob_config = Arc::new(config);
    let cache = Arc::clone(&glob_cache);
    let config = Arc::clone(&glob_config);
    let https = hyper_tls::HttpsConnector::new();
    let client = hyper::Client::builder().build::<_, hyper::Body>(https);
    let make_svc = make_service_fn(move |_conn| {
        let cache = Arc::clone(&cache);
        let config = Arc::clone(&config);
        let client = client.clone();
        async move {
            Ok::<_, tokio::task::JoinError>(service_fn(move |req| {
                let cache = Arc::clone(&cache);
                let config = Arc::clone(&config);
                let client = client.clone();

                async {
                    let handle = tokio::spawn(handle_request(config, req, cache, client));
                    let val: Result<hyper::Response<hyper::Body>, tokio::task::JoinError> =
                        match handle.await {
                            Ok(value) => match value {
                                Ok(value) => Ok(value),
                                Err(err) => Ok(err), // we want to send error responses, rather than drop the connection
                            },
                            Err(err) => Err(err),
                        };
                    val
                }
            }))
        }
    });

    let server = Server::bind(&glob_config.addr).serve(make_svc);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
            glob_config.clear_cache_interval_in_seconds,
        ));
        loop {
            interval.tick().await;
            let mut cache = glob_cache.lock().await;
            cache.0.clear();
            cache.1.clear();
        }
    });
}
