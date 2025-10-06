use crate::api::{
    CreateEndpointRequest, CreateNetworkRequest, DeleteEndpointRequest, DeleteNetworkRequest,
    ErrorResponse, JoinRequest, LeaveRequest, Validate,
};
use crate::errors::Error;
use crate::service::NetworkPluginService;
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::net::UnixListener;

use http_body_util::Full;

use http_body_util::Empty;

use log::log_enabled;
use serde_json::json;

struct HttpService {
    service: NetworkPluginService,
}

impl HttpService {
    fn new(service: NetworkPluginService) -> Self {
        Self { service }
    }

    async fn serve(
        self: Arc<Self>,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
        log::debug!(
            method = req.method().as_str(),
            path = req.uri().path();
            "Received request"
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
                Ok(Response::new(full(r#"{"Value": {}}"#)))
            }
            (&Method::POST, "/NetworkDriver.Join") => self.join(req).await,
            (&Method::POST, "/NetworkDriver.Leave") => self.leave(req).await,
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
        let body: Body<CreateNetworkRequest> = parse_request(req).await?;
        let options = body.validate()?;
        self.service.create_network(options).await?;
        Ok(Response::new(full("{}")))
    }

    async fn delete_network(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body: Body<DeleteNetworkRequest> = parse_request(req).await?;
        let options = body.validate()?;
        self.service.delete_network(options).await?;
        Ok(Response::new(full("{}")))
    }

    async fn create_endpoint(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body: Body<CreateEndpointRequest> = parse_request(req).await?;
        let options = body.validate()?;
        let config = self.service.create_endpoint(options).await?;
        if let Some(address) = config.address() {
            let (address, address_ipv6) = match address.ip() {
                std::net::IpAddr::V4(_) => (Some(address.to_string()), None),
                std::net::IpAddr::V6(_) => (None, Some(address.to_string())),
            };
            let response_json = json!({
                "Interface": {
                    "Address": address,
                    "AddressIPv6": address_ipv6,
                    "MacAddress": null,
                }
            });
            Ok(Response::new(full(response_json.to_string())))
        } else {
            Ok(Response::new(full(r#"{"Interface":{}}"#)))
        }
    }

    async fn delete_endpoint(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let _: Body<DeleteEndpointRequest> = parse_request(req).await?;
        // We don't do anything here. Cleanup is done on leave.
        Ok(Response::new(full("{}")))
    }

    async fn join(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body: Body<JoinRequest> = parse_request(req).await?;
        let options = body.validate()?;
        let interface = self.service.setup_container(options).await?;
        let static_routes: Vec<_> = interface
            .routes
            .iter()
            .map(|route| {
                json!({
                    "Destination": route.to_string(),
                    "RouteType": 1,
                })
            })
            .collect();
        let response_json = json!({
            "InterfaceName": {
                "SrcName": &interface.if_name,
                "DstPrefix": "wg",
            },
            "StaticRoutes": static_routes,
            "DisableGatewayService": true,
        });
        Ok(Response::new(full(response_json.to_string())))
    }

    async fn leave(
        &self,
        req: Request<hyper::body::Incoming>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Error> {
        let body: Body<LeaveRequest> = parse_request(req).await?;
        let options = body.validate()?;
        self.service.teardown_container(options).await?;
        Ok(Response::new(full("{}")))
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

#[derive(Debug)]
struct Body<T> {
    bytes: Bytes,
    _marker: PhantomData<T>,
}

impl<T> Body<T> {
    fn new(bytes: Bytes) -> Self {
        Self {
            bytes,
            _marker: PhantomData,
        }
    }

    fn parse_json<'s>(&'s self) -> Result<T, Error>
    where
        T: serde::de::Deserialize<'s>,
    {
        serde_json::from_slice(&self.bytes).map_err(Error::from)
    }

    fn validate<'s, O>(&'s self) -> Result<T::Output, Error>
    where
        T: serde::de::Deserialize<'s> + Validate<Output = O>,
        Error: From<T::Error>,
    {
        let value = self.parse_json()?;
        let validated = value.validate().map_err(Error::from)?;
        Ok(validated)
    }
}

async fn parse_request<T>(req: Request<hyper::body::Incoming>) -> Result<Body<T>, Error>
where
    T: serde::de::Deserialize<'static>,
{
    let body_bytes = req.collect().await?.to_bytes();
    if log_enabled!(log::Level::Trace) {
        if let Ok(s) = std::str::from_utf8(&body_bytes) {
            log::trace!(body = s; "request");
        } else {
            log::trace!(body:? = body_bytes; "request");
        }
    }
    Ok(Body::new(body_bytes))
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

pub(crate) async fn server(
    path: &str,
    service: NetworkPluginService,
    mut shutdown: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = UnixListener::bind(path)?;
    log::info!(path; "Listening on socket");

    let server = Arc::new(HttpService::new(service));

    loop {
        tokio::select! {
            Ok((stream, _addr)) = listener.accept() => {
                let io = TokioIo::new(stream);
                let server = server.clone();
                tokio::task::spawn(async move {
                    if let Err(err) = http1::Builder::new()
                        .serve_connection(io, service_fn(move |req| server.clone().serve(req)))
                        .await
                    {
                        log::error!("Error serving connection: {:?}", err);
                    }
                });
            }
            _ = &mut shutdown => {
                log::info!("Shutting down...");
                break Ok(());
            }
        }
    }
}
