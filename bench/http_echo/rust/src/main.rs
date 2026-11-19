// http_echo: POST /echo answers its body in upper case (hyper on tokio;
// TOKIO_WORKER_THREADS sets the thread count); any other request is a 404.
// bench/run.py measures it with load/main.rs.
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::convert::Infallible;
use tokio::net::TcpListener;

async fn echo(req: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    if req.method() != Method::POST || req.uri().path() != "/echo" {
        return Ok(Response::builder().status(StatusCode::NOT_FOUND).header("Content-Type", "text/plain; charset=utf-8").body(Full::new(Bytes::from_static(b"not found"))).unwrap());
    }
    let body = req.into_body().collect().await.map(|b| b.to_bytes()).unwrap_or_default();
    let up: Vec<u8> = body.iter().map(|b| b.to_ascii_uppercase()).collect();
    Ok(Response::builder().header("Content-Type", "text/plain; charset=utf-8").body(Full::new(Bytes::from(up))).unwrap())
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("127.0.0.1:18432").await.unwrap();
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        stream.set_nodelay(true).unwrap();
        tokio::spawn(async move {
            let _ = http1::Builder::new().pipeline_flush(true).serve_connection(TokioIo::new(stream), service_fn(echo)).await;
        });
    }
}
