// #![deny(warnings)]

use std::sync::Arc;

use api::ErrorResponse;
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::UnixListener;
use wg::WgError;

mod api;
mod db;
mod wg;

struct NetworkPluginService {
    db: Arc<db::Db>,
    wg: wg::Wg,
    config_provider: wg::ConfigProvider,
}

impl NetworkPluginService {
    fn new(
        db_path: impl AsRef<std::path::Path>,
        config_provider: wg::ConfigProvider,
    ) -> Result<Self, std::io::Error> {
        let db = Arc::new(db::open(db_path)?);
        let wg = wg::Wg::new().expect("Failed to create WireGuard client");
        Ok(Self {
            db,
            wg,
            config_provider,
        })
    }

    async fn serve(
        self: Arc<Self>,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
        eprintln!(
            "wireguard-docker-plugin: {} {}",
            req.method(),
            req.uri().path()
        );
        ok_or_error_response(match (req.method(), req.uri().path()) {
            (&Method::GET, "/") => Ok(Response::new(full("Ready."))),

            (&Method::POST, "/Plugin.Activate") => {
                Ok(Response::new(full(r#"{"Implements": ["NetworkDriver"]}"#)))
            }

            (&Method::POST, "/NetworkDriver.GetCapabilities") => Ok(Response::new(full(
                r#"{"Scope": "local", "ConnectivityScope": "local"} "#,
            ))),

            (&Method::POST, "/NetworkDriver.CreateNetwork") => self.create_network(req).await,

            (&Method::POST, "/NetworkDriver.DeleteNetwork") => self.delete_network(req).await,

            (&Method::POST, "/NetworkDriver.CreateEndpoint") => self.create_endpoint(req).await,

            (&Method::POST, "/NetworkDriver.DeleteEndpoint") => self.delete_endpoint(req).await,

            (&Method::POST, "/NetworkDriver.EndpointOperInfo") => {
                let mut not_found = Response::new(empty());
                *not_found.status_mut() = StatusCode::NOT_IMPLEMENTED;
                Ok(not_found)
            }

            (&Method::POST, "/NetworkDriver.Join") => self.join(req).await,

            (&Method::POST, "/NetworkDriver.Leave") => {
                let mut not_found = Response::new(empty());
                *not_found.status_mut() = StatusCode::NOT_IMPLEMENTED;
                Ok(not_found)
            }

            (&Method::POST, "/NetworkDriver.DiscoverNew") => {
                let mut not_found = Response::new(empty());
                *not_found.status_mut() = StatusCode::NOT_IMPLEMENTED;
                Ok(not_found)
            }

            (&Method::POST, "/NetworkDriver.DiscoverDelete") => {
                let mut not_found = Response::new(empty());
                *not_found.status_mut() = StatusCode::NOT_IMPLEMENTED;
                Ok(not_found)
            }

            _ => {
                let mut not_found = Response::new(empty());
                *not_found.status_mut() = StatusCode::NOT_FOUND;
                Ok(not_found)
            }
        })
    }

    async fn create_network(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body_bytes = req.collect().await?.to_bytes();
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let req_body: api::CreateNetworkRequest =
                serde_json::from_slice(&body_bytes).map_err(Error::from)?;

            let mut missing_fields = vec![];
            if req_body.options.generic.config.is_none() {
                missing_fields.push("wireguard-config");
            }
            if !missing_fields.is_empty() {
                return Err(Error::MissingConfig(missing_fields));
            }

            let config = req_body.options.generic.config.unwrap().to_owned();

            let network_id = req_body.network_id;

            db.create_network(network_id, config).map_err(Error::from)
        })
        .await??;
        Ok(Response::new(full("{}")))
    }

    async fn delete_network(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body_bytes = req.collect().await?.to_bytes();
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let req_body: api::DeleteNetworkRequest =
                serde_json::from_slice(&body_bytes).map_err(Error::from)?;

            let network_id = req_body.network_id;

            db.delete_network(network_id).map_err(Error::from)
        })
        .await??;
        Ok(Response::new(full("{}")))
    }

    async fn create_endpoint(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body_bytes = req.collect().await?.to_bytes();
        if let Ok(s) = std::str::from_utf8(&body_bytes) {
            eprintln!("wireguard-docker-plugin: {}", s);
        }
        Ok(Response::new(full(r#"{"interface":{}}"#)))
    }

    async fn delete_endpoint(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body_bytes = req.collect().await?.to_bytes();
        if let Ok(s) = std::str::from_utf8(&body_bytes) {
            eprintln!("wireguard-docker-plugin: {}", s);
        }
        Ok(Response::new(full("{}")))
    }

    async fn join(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body_bytes = req.collect().await?.to_bytes();
        if let Ok(s) = std::str::from_utf8(&body_bytes) {
            eprintln!("wireguard-docker-plugin: {}", s);
        }
        let db = self.db.clone();
        let network = tokio::task::spawn_blocking(move || -> Result<_, Error> {
            let req_body: api::JoinRequest =
                serde_json::from_slice(&body_bytes).map_err(Error::from)?;
            db.get_network(req_body.network_id).map_err(Error::from)
        })
        .await??;
        let config_name = network.config();
        let config = self.config_provider.get_config(config_name).await?;
        self.wg.create_interface(config_name, config).await?;
        Ok(Response::new(full(r#"{"interface":{}}"#)))
    }
}

fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

enum Error {
    Hyper(hyper::Error),
    SerdeJson(serde_json::Error),
    Io(std::io::Error),
    Wg(WgError),
    MissingConfig(Vec<&'static str>),
    Abort,
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::SerdeJson(e)
    }
}

impl From<hyper::Error> for Error {
    fn from(e: hyper::Error) -> Self {
        Error::Hyper(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<tokio::task::JoinError> for Error {
    fn from(_: tokio::task::JoinError) -> Self {
        Error::Abort
    }
}

impl From<WgError> for Error {
    fn from(e: WgError) -> Self {
        Error::Wg(e)
    }
}

fn ok_or_error_response(
    result: Result<Response<BoxBody<Bytes, hyper::Error>>, Error>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    Ok(match result {
        Ok(response) => response,
        Err(Error::Hyper(e)) => return Err(e),
        Err(Error::SerdeJson(e)) => {
            let message = format!("Invalid request: {e}");
            error_response(&message, StatusCode::BAD_REQUEST)
        }
        Err(Error::Io(e)) => {
            let message = e.to_string();
            error_response(&message, StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(Error::MissingConfig(fields)) => {
            let message = format!("Missing configuration options: {}", &fields.join(", "));
            error_response(&message, StatusCode::BAD_REQUEST)
        }
        Err(Error::Wg(e)) => {
            let message = format!("error while configuring wireguard interface: {e}");
            error_response(&message, StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(Error::Abort) => error_response("aborted", StatusCode::INTERNAL_SERVER_ERROR),
    })
}

fn error_response(
    message: &str,
    status_code: StatusCode,
) -> Response<BoxBody<Bytes, hyper::Error>> {
    let err_response = ErrorResponse::new(message);
    let body = serde_json::to_vec(&err_response).unwrap();
    let mut response = Response::new(full(body));
    *response.status_mut() = status_code;
    response
}

async fn shutdown_signal() {
    // Wait for the CTRL+C signal
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install CTRL+C signal handler");
}

async fn server(
    path: &str,
    service: Arc<NetworkPluginService>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = UnixListener::bind(path)?;
    println!("Listening on {path}");
    let mut shutdown = std::pin::pin!(shutdown_signal());

    loop {
        tokio::select! {
            Ok((stream, _addr)) = listener.accept() => {
                let io = TokioIo::new(stream);
                let service_ref = service.clone();
                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, service_fn(move |req| service_ref.clone().serve(req)))
                        .await
                    {
                        println!("Error serving connection: {:?}", err);
                    }
                });
            }
            _ = &mut shutdown => {
                println!("Shutting down...");
                break Ok(());
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let socket_path = "/run/docker/plugins/wireguard.sock";
    let db_path = "wireguard_db";
    let conf_path = "wireguard_conf";
    let config_provider = wg::ConfigProvider::new_file(conf_path.into());

    let service = Arc::new(NetworkPluginService::new(db_path, config_provider)?);

    server(socket_path, service).await?;

    if std::fs::remove_file(socket_path).is_ok() {
        println!("Removed socket file");
    }

    Ok(())
}
